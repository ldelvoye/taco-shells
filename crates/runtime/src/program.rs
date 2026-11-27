use std::io::{BufWriter, ErrorKind, Write};
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Instant;

use taco_protocol::{RuntimeCommand, RuntimeReport, TerminalEvent, TerminalId, TerminalRequest};

use crate::budget::Budgets;
use crate::core::{Core, Effect, Input};
use crate::pty::{PtyShells, ShellProgram};

enum Waited {
    Input(Input),
    Deadline,
    Closed,
}

/// The runtime, running until it is told to stop, the app's connection closes, or SIGTERM arrives; SIGTERM must already be blocked in every thread.
pub fn run(
    control: UnixStream,
    terminals: UnixStream,
    program: ShellProgram,
) -> std::io::Result<()> {
    let (inputs, received) = mpsc::channel::<Input>();
    let budgets = Arc::new(Budgets::default());

    let reports = spawn_report_writer(control.try_clone()?)?;
    let events = spawn_event_writer(terminals.try_clone()?, budgets.clone())?;
    spawn_command_reader(control, inputs.clone())?;
    spawn_request_reader(terminals, inputs.clone())?;
    spawn_signal_waiter(inputs.clone())?;

    let shells = PtyShells::new(program, inputs, budgets.clone());
    let mut core = Core::new(shells);
    drive(&mut core, &received, &reports, &events, &budgets);
    Ok(())
}

fn drive(
    core: &mut Core<PtyShells>,
    received: &Receiver<Input>,
    reports: &Sender<RuntimeReport>,
    events: &Sender<TerminalEvent>,
    budgets: &Budgets,
) {
    loop {
        let deadline = core.next_deadline();
        let waited = next_input(received, deadline);
        let now = Instant::now();
        let mut exited = None;
        let effects = match waited {
            Waited::Input(input) => {
                if let Input::Exited { id, .. } = &input {
                    exited = Some(id.clone());
                }
                core.handle(input, now)
            }
            Waited::Deadline => core.tick(now),
            Waited::Closed => return,
        };

        for effect in effects {
            match effect {
                Effect::Report(report) => {
                    let _ = reports.send(report);
                }
                Effect::Event(event) => {
                    let _ = events.send(event);
                }
                Effect::Parsed { id, bytes } => budgets.give_back(&id, bytes),
                Effect::Stop => {
                    core.shells_mut().stop_all();
                    return;
                }
            }
        }
        if let Some(id) = exited {
            core.shells_mut().forget(&id);
        }
    }
}

fn next_input(received: &Receiver<Input>, deadline: Option<Instant>) -> Waited {
    let Some(deadline) = deadline else {
        return match received.recv() {
            Ok(input) => Waited::Input(input),
            Err(_) => Waited::Closed,
        };
    };
    let wait = deadline.saturating_duration_since(Instant::now());
    match received.recv_timeout(wait) {
        Ok(input) => Waited::Input(input),
        Err(RecvTimeoutError::Timeout) => Waited::Deadline,
        Err(RecvTimeoutError::Disconnected) => Waited::Closed,
    }
}

fn spawn_command_reader(mut control: UnixStream, inputs: Sender<Input>) -> std::io::Result<()> {
    thread::Builder::new()
        .name("read commands".into())
        .spawn(move || {
            loop {
                let command = match taco_wire::read_frame::<RuntimeCommand>(&mut control) {
                    Ok(Some(command)) => command,
                    Ok(None) => RuntimeCommand::Stop,
                    Err(error) => {
                        eprintln!("the app's connection failed: {error}");
                        RuntimeCommand::Stop
                    }
                };
                // A closed app connection, however it closed, stops the runtime like a stop request.
                let stopping = command == RuntimeCommand::Stop;
                if inputs.send(Input::Command(command)).is_err() || stopping {
                    return;
                }
            }
        })?;
    Ok(())
}

fn spawn_request_reader(mut terminals: UnixStream, inputs: Sender<Input>) -> std::io::Result<()> {
    thread::Builder::new()
        .name("read requests".into())
        .spawn(move || {
            // Unlike the app's connection, the window's closing stops nothing: the app decides when the runtime stops.
            while let Ok(Some(request)) = taco_wire::read_frame::<TerminalRequest>(&mut terminals) {
                if inputs.send(Input::Request(request)).is_err() {
                    return;
                }
            }
        })?;
    Ok(())
}

fn spawn_report_writer(control: UnixStream) -> std::io::Result<Sender<RuntimeReport>> {
    let (reports, queued) = mpsc::channel::<RuntimeReport>();
    thread::Builder::new()
        .name("write reports".into())
        .spawn(move || {
            let mut writer = BufWriter::new(control);
            for report in queued {
                if taco_wire::write_frame(&mut writer, &report).is_err() || writer.flush().is_err()
                {
                    return;
                }
            }
        })?;
    Ok(reports)
}

fn spawn_event_writer(
    terminals: UnixStream,
    budgets: Arc<Budgets>,
) -> std::io::Result<Sender<TerminalEvent>> {
    let (events, queued) = mpsc::channel::<TerminalEvent>();
    thread::Builder::new()
        .name("write events".into())
        .spawn(move || {
            let mut writer = BufWriter::new(terminals);
            while let Ok(first) = queued.recv() {
                let mut batch = vec![first];
                while let Ok(next) = queued.try_recv() {
                    batch.push(next);
                }
                let merged = merge_output(batch);
                for event in merged {
                    let written = taco_wire::write_frame(&mut writer, &event);
                    // A window that has gone is not an error here: its shells keep running. An event over the frame limit is, and is reported, since its view would wait for it in silence.
                    if let Err(error) = written
                        && error.kind() == ErrorKind::InvalidInput
                    {
                        let id = terminal_of(&event);
                        eprintln!("an event for {} was not sent: {error}", id.0);
                    }
                    // Bytes sent to a view return to their shell's budget even when unwritten, so a gone window cannot stall a shell.
                    if let TerminalEvent::Output { id, data } = &event {
                        budgets.give_back(id, data.len());
                    }
                }
                let _ = writer.flush();
            }
        })?;
    Ok(events)
}

/// A burst of output becomes a few large frames instead of one per read, while a lone keystroke's echo goes out at once.
fn merge_output(batch: Vec<TerminalEvent>) -> Vec<TerminalEvent> {
    let mut merged: Vec<TerminalEvent> = Vec::new();
    for event in batch {
        if let TerminalEvent::Output { id, data } = &event
            && let Some(TerminalEvent::Output {
                id: previous_id,
                data: previous_data,
            }) = merged.last_mut()
            && previous_id == id
        {
            previous_data.extend_from_slice(data);
            continue;
        }
        merged.push(event);
    }
    merged
}

fn terminal_of(event: &TerminalEvent) -> &TerminalId {
    match event {
        TerminalEvent::Attached { id, .. }
        | TerminalEvent::History { id, .. }
        | TerminalEvent::Output { id, .. }
        | TerminalEvent::Resized { id, .. }
        | TerminalEvent::Cleared { id }
        | TerminalEvent::CursorDefaults { id, .. }
        | TerminalEvent::End { id } => id,
    }
}

fn spawn_signal_waiter(inputs: Sender<Input>) -> std::io::Result<()> {
    thread::Builder::new()
        .name("wait for SIGTERM".into())
        .spawn(move || {
            let set = termination_signals();
            let mut signal = 0;
            unsafe { libc::sigwait(&set, &mut signal) };
            let _ = inputs.send(Input::Command(RuntimeCommand::Stop));
        })?;
    Ok(())
}

/// One of the connections the app handed over, at `fd`, or none if `fd` is a standard stream or not open.
pub fn adopt_connection(fd: RawFd) -> Option<UnixStream> {
    if fd <= libc::STDERR_FILENO {
        return None;
    }
    // Inherited without close-on-exec, so set here: otherwise every shell would hold the app's connections.
    let marked = unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    if marked < 0 {
        return None;
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    Some(UnixStream::from(owned))
}

/// The signals a stop arrives as. Blocked before any thread starts, so only the waiter receives them.
pub fn termination_signals() -> libc::sigset_t {
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use taco_protocol::TerminalId;

    #[test]
    fn consecutive_output_of_one_terminal_is_sent_as_one_frame() {
        let a = TerminalId("a".into());
        let b = TerminalId("b".into());
        let output = |id: &TerminalId, data: &[u8]| TerminalEvent::Output {
            id: id.clone(),
            data: data.to_vec(),
        };
        let batch = vec![
            output(&a, b"1"),
            output(&a, b"2"),
            output(&b, b"x"),
            TerminalEvent::Cleared { id: a.clone() },
            output(&a, b"3"),
            output(&a, b"4"),
        ];
        let merged = merge_output(batch);
        let expected = vec![
            output(&a, b"12"),
            output(&b, b"x"),
            TerminalEvent::Cleared { id: a.clone() },
            output(&a, b"34"),
        ];
        assert_eq!(merged, expected);
    }
}

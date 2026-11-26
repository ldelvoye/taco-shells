use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use taco_ghostty::{Snapshot, Step, Terminal};
use taco_protocol::{
    CellSize, Rgb, RuntimeAnswer, RuntimeCommand, RuntimeQuery, RuntimeReport, TerminalColors,
    TerminalEvent, TerminalId, TerminalRequest, TerminalSize, TerminalStart,
};

use crate::core::{Core, Effect, Input};
use crate::shells::Shells;

const SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };
const NO_CELL: CellSize = CellSize {
    width_px: 0,
    height_px: 0,
};
const HOME: &str = "/Users/someone";

#[derive(Default)]
struct FakeShells {
    started: Vec<(TerminalId, Option<String>)>,
    written: Vec<(TerminalId, Vec<u8>)>,
    resized: Vec<(TerminalId, TerminalSize)>,
    directories: HashMap<TerminalId, String>,
    busy: HashSet<TerminalId>,
    refuse: HashSet<TerminalId>,
    pids: HashMap<TerminalId, u32>,
}

impl Shells for FakeShells {
    fn start(
        &mut self,
        id: &TerminalId,
        directory: Option<&str>,
        _size: TerminalSize,
    ) -> Option<String> {
        let requested = directory.map(String::from);
        self.started.push((id.clone(), requested.clone()));
        if self.refuse.contains(id) {
            return None;
        }
        let used = match requested {
            Some(directory) => directory,
            None => HOME.to_string(),
        };
        self.directories.insert(id.clone(), used.clone());
        Some(used)
    }
    fn write(&mut self, id: &TerminalId, data: &[u8]) {
        self.written.push((id.clone(), data.to_vec()));
    }
    fn resize(&mut self, id: &TerminalId, size: TerminalSize) {
        self.resized.push((id.clone(), size));
    }
    fn close(&mut self, _id: &TerminalId) {}
    fn directory(&self, id: &TerminalId) -> Option<String> {
        self.directories.get(id).cloned()
    }
    fn at_prompt(&self, id: &TerminalId) -> bool {
        !self.busy.contains(id)
    }
    fn foreground_commands(&self, _id: &TerminalId) -> Vec<String> {
        Vec::new()
    }
    fn shell_pid(&self, id: &TerminalId) -> Option<u32> {
        self.pids.get(id).copied()
    }
}

fn id(name: &str) -> TerminalId {
    TerminalId(name.to_string())
}

fn start(name: &str) -> TerminalStart {
    TerminalStart {
        id: id(name),
        directory: None,
        directory_of: None,
        command: None,
        size: SIZE,
    }
}

fn opened(names: &[&str]) -> (Core<FakeShells>, Instant) {
    let mut core = Core::new(FakeShells::default());
    let now = Instant::now();
    for name in names {
        let open = RuntimeCommand::Open(start(name));
        core.handle(Input::Command(open), now);
    }
    (core, now)
}

fn output(core: &mut Core<FakeShells>, name: &str, data: &[u8], now: Instant) -> Vec<Effect> {
    let input = Input::Output {
        id: id(name),
        data: data.to_vec(),
    };
    core.handle(input, now)
}

fn request(core: &mut Core<FakeShells>, request: TerminalRequest, now: Instant) -> Vec<Effect> {
    core.handle(Input::Request(request), now)
}

fn events(effects: &[Effect]) -> Vec<&TerminalEvent> {
    let mut events = Vec::new();
    for effect in effects {
        if let Effect::Event(event) = effect {
            events.push(event);
        }
    }
    events
}

fn reports(effects: &[Effect]) -> Vec<&RuntimeReport> {
    let mut reports = Vec::new();
    for effect in effects {
        if let Effect::Report(report) = effect {
            reports.push(report);
        }
    }
    reports
}

/// What a view would end up with after applying these events in order.
fn apply(events: &[&TerminalEvent], view: &mut Option<Terminal>) {
    for event in events {
        match event {
            TerminalEvent::Attached { screen, .. } => {
                *view = Some(Terminal::restore(screen).unwrap())
            }
            TerminalEvent::History { part, last, .. } => {
                let terminal = view.as_mut().unwrap();
                terminal.push_history(part, *last);
                while terminal.step_history().unwrap() == Step::Applied {}
            }
            TerminalEvent::Output { data, .. } => view.as_mut().unwrap().write(data),
            TerminalEvent::Resized { size, .. } => {
                view.as_mut().unwrap().resize(*size, 0, 0).unwrap()
            }
            TerminalEvent::Cleared { .. } => view.as_mut().unwrap().clear().unwrap(),
            TerminalEvent::End { .. } => {}
        }
    }
}

fn encoded(terminal: &Terminal) -> Vec<u8> {
    let snapshot = Snapshot::take(terminal).unwrap();
    let mut bytes = snapshot.screen().to_vec();
    for part in snapshot.history() {
        bytes.extend_from_slice(part);
    }
    bytes
}

fn kind(event: &TerminalEvent) -> &'static str {
    match event {
        TerminalEvent::Attached { .. } => "attached",
        TerminalEvent::History { .. } => "history",
        TerminalEvent::Output { .. } => "output",
        TerminalEvent::Resized { .. } => "resized",
        TerminalEvent::Cleared { .. } => "cleared",
        TerminalEvent::End { .. } => "end",
    }
}

#[test]
fn a_view_gets_the_output_before_its_attach_in_the_snapshot_and_follows_in_order_after() {
    let mut core = Core::new(FakeShells::default());
    let now = Instant::now();
    // Pushed before the terminal opens, so it is the one the new screen starts with.
    let scrollback = RuntimeCommand::Scrollback { lines: 40_000 };
    core.handle(Input::Command(scrollback), now);
    let open = RuntimeCommand::Open(start("a"));
    core.handle(Input::Command(open), now);

    let mut earlier = Vec::new();
    for n in 0..30_000 {
        let line = format!(
            "\x1b[3{}mline {n} of the output before any view\x1b[0m\r\n",
            n % 8
        );
        earlier.extend_from_slice(line.as_bytes());
    }
    output(&mut core, "a", &earlier, now);
    output(&mut core, "a", b"prompt $ \x1b[3", now);

    let wider = TerminalSize {
        cols: 100,
        rows: 30,
    };
    let attach = TerminalRequest::Attach {
        id: id("a"),
        attachment: 7,
        size: Some(wider),
        cell: None,
    };
    let mut effects = request(&mut core, attach, now);
    effects.extend(output(&mut core, "a", b"1mred\x1b[0m after", now));
    let narrower = TerminalRequest::Resize {
        id: id("a"),
        size: SIZE,
        cell: NO_CELL,
    };
    effects.extend(request(&mut core, narrower.clone(), now));
    effects.extend(request(&mut core, narrower, now));
    let clear = TerminalRequest::Clear { id: id("a") };
    effects.extend(request(&mut core, clear, now));
    effects.extend(output(&mut core, "a", b"\r\nnext", now));

    let sent = events(&effects);
    let kinds: Vec<&str> = sent.iter().map(|event| kind(event)).collect();
    let mut lasts = Vec::new();
    for event in &sent {
        if let TerminalEvent::History { last, .. } = event {
            lasts.push(*last);
        }
    }
    assert!(
        lasts.len() >= 2,
        "30,000 lines of history take more than one part"
    );
    let (final_part, earlier_parts) = lasts.split_last().unwrap();
    assert!(
        *final_part && !earlier_parts.contains(&true),
        "only the final part is the last: {lasts:?}"
    );

    let mut expected = vec!["attached"];
    expected.extend(std::iter::repeat_n("history", lasts.len()));
    expected.extend(["output", "resized", "cleared", "output"]);
    assert_eq!(
        kinds, expected,
        "the second resize to the same size is not one"
    );
    let TerminalEvent::Attached {
        attachment, size, ..
    } = sent[0]
    else {
        unreachable!()
    };
    assert_eq!(
        (*attachment, *size),
        (7, wider),
        "the snapshot already has the size the attach asked for"
    );

    let mut view = None;
    apply(&sent, &mut view);
    let runtime_side = core.terminal(&id("a")).unwrap();
    assert_eq!(encoded(view.as_ref().unwrap()), encoded(runtime_side));
    assert_eq!(
        core.shells_mut().resized,
        vec![(id("a"), wider), (id("a"), SIZE)]
    );
}

#[test]
fn only_the_newest_attach_of_a_terminal_is_sent_anything() {
    let (mut core, now) = opened(&["a", "b"]);
    assert!(events(&output(&mut core, "a", b"before any view", now)).is_empty());

    request(
        &mut core,
        TerminalRequest::Attach {
            id: id("a"),
            attachment: 1,
            size: None,
            cell: None,
        },
        now,
    );
    // A new size with the new attach: the view being replaced is not told about it.
    let wider = TerminalSize {
        cols: 120,
        rows: 40,
    };
    let reattach = TerminalRequest::Attach {
        id: id("a"),
        attachment: 2,
        size: Some(wider),
        cell: None,
    };
    let again = request(&mut core, reattach, now);
    for event in events(&again) {
        match event {
            TerminalEvent::Attached { attachment, .. }
            | TerminalEvent::History { attachment, .. } => assert_eq!(*attachment, 2),
            other => panic!("unexpected {other:?}"),
        }
    }

    request(&mut core, TerminalRequest::Detach { id: id("a") }, now);
    let after_detach = output(&mut core, "a", b"nobody watching", now);
    assert!(events(&after_detach).is_empty());
    assert!(events(&output(&mut core, "b", b"never attached", now)).is_empty());
}

#[test]
fn output_goes_back_to_the_shell_budget_now_only_when_no_view_will_write_it() {
    let (mut core, now) = opened(&["watched", "unwatched"]);
    request(
        &mut core,
        TerminalRequest::Attach {
            id: id("watched"),
            attachment: 1,
            size: None,
            cell: None,
        },
        now,
    );

    let watched = output(&mut core, "watched", b"0123456789", now);
    assert!(
        !watched
            .iter()
            .any(|effect| matches!(effect, Effect::Parsed { .. }))
    );
    assert!(
        events(&watched)
            .iter()
            .any(|event| matches!(event, TerminalEvent::Output { .. }))
    );

    let unwatched = output(&mut core, "unwatched", b"0123456789", now);
    assert!(unwatched.contains(&Effect::Parsed {
        id: id("unwatched"),
        bytes: 10
    }));
}

#[test]
fn an_exit_ends_the_view_after_the_last_output_and_the_terminal_is_gone() {
    let (mut core, now) = opened(&["a"]);
    request(
        &mut core,
        TerminalRequest::Attach {
            id: id("a"),
            attachment: 1,
            size: None,
            cell: None,
        },
        now,
    );
    let mut effects = output(&mut core, "a", b"logout\r\n", now);
    effects.extend(core.handle(
        Input::Exited {
            id: id("a"),
            code: 3,
        },
        now,
    ));

    let kinds: Vec<&str> = events(&effects).iter().map(|event| kind(event)).collect();
    assert_eq!(kinds, ["output", "end"]);
    assert!(reports(&effects).contains(&&RuntimeReport::Exited {
        id: id("a"),
        code: 3
    }));

    let late = request(
        &mut core,
        TerminalRequest::Attach {
            id: id("a"),
            attachment: 2,
            size: None,
            cell: None,
        },
        now,
    );
    assert!(late.is_empty());
}

#[test]
fn opening_starts_one_shell_per_id_where_it_was_asked() {
    let (mut core, now) = opened(&["first"]);
    core.shells_mut()
        .directories
        .insert(id("first"), "/repo/subdir".into());

    let again = core.handle(Input::Command(RuntimeCommand::Open(start("first"))), now);
    assert!(again.is_empty());

    let mut beside = start("beside");
    beside.directory_of = Some(id("first"));
    beside.command = Some("claude --resume x\r".into());
    let effects = core.handle(Input::Command(RuntimeCommand::Open(beside)), now);
    assert_eq!(
        reports(&effects),
        [&RuntimeReport::Started {
            id: id("beside"),
            directory: "/repo/subdir".into()
        }]
    );
    let typed = core.shells_mut().written.last().unwrap().clone();
    assert_eq!(typed, (id("beside"), b"claude --resume x\r".to_vec()));

    core.shells_mut().refuse.insert(id("broken"));
    let refused = core.handle(Input::Command(RuntimeCommand::Open(start("broken"))), now);
    assert_eq!(
        reports(&refused),
        [&RuntimeReport::Exited {
            id: id("broken"),
            code: 1
        }]
    );

    let started: Vec<TerminalId> = core
        .shells_mut()
        .started
        .iter()
        .map(|(id, _)| id.clone())
        .collect();
    assert_eq!(started, [id("first"), id("beside"), id("broken")]);
}

#[test]
fn a_title_is_reported_only_when_it_changes() {
    let (mut core, now) = opened(&["a"]);
    let first = output(&mut core, "a", b"\x1b]2;claude\x07", now);
    assert_eq!(
        reports(&first),
        [&RuntimeReport::Titled {
            id: id("a"),
            title: "claude".into()
        }]
    );
    assert!(reports(&output(&mut core, "a", b"\x1b]2;claude\x07", now)).is_empty());
    let renamed = output(&mut core, "a", b"\x1b]0;claude: tests\x07", now);
    let expected = RuntimeReport::Titled {
        id: id("a"),
        title: "claude: tests".into(),
    };
    assert_eq!(reports(&renamed), [&expected]);
}

#[test]
fn a_directory_is_read_at_the_prompt_at_most_every_200_ms_and_reported_on_a_change() {
    let (mut core, start_time) = opened(&["a"]);
    let changed = |effects: &[Effect]| -> Vec<String> {
        let mut directories = Vec::new();
        for report in reports(effects) {
            if let RuntimeReport::DirectoryChanged { directory, .. } = report {
                directories.push(directory.clone());
            }
        }
        directories
    };

    assert!(
        changed(&output(&mut core, "a", b"$ ", start_time)).is_empty(),
        "still where it started"
    );

    core.shells_mut().directories.insert(id("a"), "/tmp".into());
    let soon = start_time + Duration::from_millis(50);
    assert!(
        changed(&output(&mut core, "a", b"cd /tmp\r\n$ ", soon)).is_empty(),
        "inside the floor"
    );
    assert_eq!(
        core.next_deadline(),
        Some(start_time + Duration::from_millis(200))
    );
    let at_deadline = start_time + Duration::from_millis(200);
    assert_eq!(changed(&core.tick(at_deadline)), ["/tmp"]);
    assert_eq!(core.next_deadline(), None);

    core.shells_mut().busy.insert(id("a"));
    core.shells_mut()
        .directories
        .insert(id("a"), "/elsewhere".into());
    let later = start_time + Duration::from_secs(1);
    assert!(
        changed(&output(&mut core, "a", b"a program printing", later)).is_empty(),
        "a running program is not the shell"
    );
}

#[test]
fn a_typed_line_is_submitted_and_the_screen_answers_programs_itself() {
    let mut core = Core::new(FakeShells::default());
    let now = Instant::now();
    // Pushed before the terminal opens, so the new screen answers from them.
    let mut colors = TerminalColors {
        foreground: Rgb {
            r: 0xd6,
            g: 0xd0,
            b: 0xc9,
        },
        background: Rgb {
            r: 0x19,
            g: 0x19,
            b: 0x18,
        },
        cursor: Rgb {
            r: 0xec,
            g: 0xe7,
            b: 0xe1,
        },
        cursor_text: Rgb {
            r: 0x19,
            g: 0x19,
            b: 0x18,
        },
        selection: Rgb {
            r: 0x3a,
            g: 0x38,
            b: 0x36,
        },
        ansi: [Rgb { r: 0, g: 0, b: 0 }; 16],
    };
    colors.ansi[1] = Rgb {
        r: 0xf0,
        g: 0x64,
        b: 0x6c,
    };
    core.handle(Input::Command(RuntimeCommand::Colors(colors)), now);
    core.handle(Input::Command(RuntimeCommand::Open(start("a"))), now);

    let line = TerminalRequest::Write {
        id: id("a"),
        data: b"ls\r".to_vec(),
    };
    let typed = request(&mut core, line, now);
    assert_eq!(reports(&typed), [&RuntimeReport::Submitted { id: id("a") }]);
    let key = TerminalRequest::Write {
        id: id("a"),
        data: b"l".to_vec(),
    };
    let unsubmitted = request(&mut core, key, now);
    assert!(reports(&unsubmitted).is_empty());

    let asked = output(&mut core, "a", b"\x1b[6n\x1b]11;?\x07", now);
    assert!(
        reports(&asked).is_empty(),
        "an answer to a program is not a submitted line"
    );
    let written = &core.shells_mut().written;
    let (to, answer) = written.last().unwrap().clone();
    assert_eq!(to, id("a"));
    assert_eq!(answer, b"\x1b[1;1R\x1b]11;rgb:1919/1919/1818\x07".to_vec());
}

#[test]
fn a_hook_belongs_to_the_terminal_of_its_nearest_shell() {
    let (mut core, now) = opened(&["outer", "inner"]);
    core.shells_mut().pids.insert(id("outer"), 100);
    core.shells_mut().pids.insert(id("inner"), 200);

    let ask = |pids: Vec<u32>| RuntimeCommand::Ask {
        ask: 9,
        query: RuntimeQuery::TerminalForPids { pids },
    };
    let nearest = core.handle(Input::Command(ask(vec![300, 200, 150, 100])), now);
    assert_eq!(
        reports(&nearest),
        [&RuntimeReport::Answer {
            ask: 9,
            value: RuntimeAnswer::Terminal(Some(id("inner")))
        }]
    );
    let none = core.handle(Input::Command(ask(vec![1, 2])), now);
    assert_eq!(
        reports(&none),
        [&RuntimeReport::Answer {
            ask: 9,
            value: RuntimeAnswer::Terminal(None)
        }]
    );
}

#[test]
fn a_size_report_in_pixels_is_answered_from_the_last_cell_size_a_view_gave() {
    let (mut core, now) = opened(&["a"]);
    let size_reports = b"\x1b[16t\x1b[14t";
    let answer = |core: &mut Core<FakeShells>| core.shells_mut().written.last().unwrap().1.clone();

    let narrow_font = CellSize {
        width_px: 9,
        height_px: 18,
    };
    let resize = TerminalRequest::Resize {
        id: id("a"),
        size: SIZE,
        cell: narrow_font,
    };
    request(&mut core, resize, now);
    output(&mut core, "a", size_reports, now);
    // A cell of 9 by 18, and 80 by 24 of them: 720 by 432.
    assert_eq!(answer(&mut core), b"\x1b[6;18;9t\x1b[4;432;720t".to_vec());

    let wide_font = CellSize {
        width_px: 10,
        height_px: 20,
    };
    let attach = TerminalRequest::Attach {
        id: id("a"),
        attachment: 1,
        size: None,
        cell: Some(wide_font),
    };
    let attached = request(&mut core, attach, now);
    output(&mut core, "a", size_reports, now);
    assert_eq!(answer(&mut core), b"\x1b[6;20;10t\x1b[4;480;800t".to_vec());

    let reattach = TerminalRequest::Attach {
        id: id("a"),
        attachment: 2,
        size: None,
        cell: None,
    };
    request(&mut core, reattach, now);
    output(&mut core, "a", size_reports, now);
    assert_eq!(
        answer(&mut core),
        b"\x1b[6;20;10t\x1b[4;480;800t".to_vec(),
        "an attach without a cell keeps the last one"
    );

    let resized: Vec<&TerminalEvent> = events(&attached)
        .into_iter()
        .filter(|event| matches!(event, TerminalEvent::Resized { .. }))
        .collect();
    assert!(resized.is_empty(), "a new cell alone is not a new size");
    assert!(
        core.shells_mut().resized.is_empty(),
        "nor does the shell hear of it"
    );
}

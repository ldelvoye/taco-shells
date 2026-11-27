//! Host threads whose panic the window shows as a banner, rather than passing unseen.

use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::thread;

use crate::ports::{ServicesOutput, WindowOutput, WindowSender};

/// Runs `body` on a thread named `name`; if it panics, the window is shown `problem`.
pub(crate) fn spawn_supervised(
    name: &str,
    problem: &'static str,
    window: WindowSender,
    body: impl FnOnce() + Send + 'static,
) -> io::Result<()> {
    let supervise = move || {
        let finished = panic::catch_unwind(AssertUnwindSafe(body));
        if finished.is_err() {
            let shown = ServicesOutput::Problem(problem.to_string());
            // a send fails only once the window is gone
            let _ = window.unbounded_send(WindowOutput::Services(shown));
        }
    };
    thread::Builder::new().name(name.into()).spawn(supervise)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use futures::channel::mpsc::unbounded;
    use futures::executor::block_on;

    use super::spawn_supervised;
    use crate::ports::{ServicesOutput, WindowOutput};

    #[test]
    fn a_thread_that_panics_says_so_and_one_that_returns_says_nothing() {
        let (window, mut shown) = unbounded();
        spawn_supervised("returns", "not shown", window.clone(), || {}).unwrap();
        spawn_supervised("panics", "it stopped", window, || panic!("on purpose")).unwrap();

        // Both threads end, and the window's last sender with them, so the stream ends after what they sent.
        let outputs: Vec<WindowOutput> = block_on(async { shown.by_ref().collect().await });
        let problems: Vec<String> = outputs
            .into_iter()
            .map(|output| match output {
                WindowOutput::Services(ServicesOutput::Problem(problem)) => problem,
                _ => panic!("only a problem is sent"),
            })
            .collect();
        assert_eq!(problems, ["it stopped"]);
    }
}

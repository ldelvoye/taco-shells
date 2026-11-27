//! A binary of its own: the test sets the process's environment, which no other test may be reading meanwhile.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use taco_protocol::{TerminalId, TerminalSize};
use taco_runtime::budget::Budgets;
use taco_runtime::core::Input;
use taco_runtime::pty::{PtyShells, ShellContext, ShellProgram};
use taco_runtime::shells::Shells;

const SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };
const PATIENCE: Duration = Duration::from_secs(5);
const REPORT: &str = r#"print "$ENV{TACO_SHELLS_APP} $ENV{TACO_SHELLS_PORT_FILE}\n";"#;

#[test]
fn a_shell_is_told_the_runtimes_own_app_over_the_one_the_runtime_inherited() {
    // As when a Dev app is opened from a stable app's terminal.
    unsafe {
        std::env::set_var("TACO_SHELLS_APP", "taco-shells");
        std::env::set_var("TACO_SHELLS_PORT_FILE", "/stable/focus-port");
    }
    let program = ShellProgram {
        path: "/usr/bin/perl".into(),
        arguments: vec!["-e".into(), REPORT.into()],
        home: "/".into(),
        open_file_limit: None,
        context: ShellContext {
            app: "taco-shells-dev".into(),
            port_file: "/dev-config/focus-port".into(),
            plugin_directory: "/dev-config/claude-plugin".into(),
        },
    };
    let (inputs, received) = mpsc::channel();
    let mut shells = PtyShells::new(program, inputs, Arc::new(Budgets::default()));
    let id = TerminalId("context".into());
    shells.start(&id, None, SIZE);

    let mut seen = String::new();
    while !seen.contains('\n') {
        let input = received.recv_timeout(PATIENCE);
        let Ok(input) = input else {
            panic!("nothing more from the shell after {seen:?}");
        };
        if let Input::Output { data, .. } = input {
            seen.push_str(&String::from_utf8_lossy(&data));
        }
    }
    assert_eq!(seen.trim(), "taco-shells-dev /dev-config/focus-port");
}

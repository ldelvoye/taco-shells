//! A binary of its own: the test sets the process's environment, which no other test may be reading meanwhile.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use taco_protocol::{TerminalId, TerminalSize};
use taco_runtime::budget::Budgets;
use taco_runtime::core::Input;
use taco_runtime::pty::{PtyShells, ShellContext, ShellProgram};
use taco_runtime::shells::Shells;

const SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };
const PATIENCE: Duration = Duration::from_secs(5);
const OWN: &str = "/dev-config/claude-plugin";
const REPORT: &str = r#"print exists $ENV{CLAUDE_CODE_PLUGIN_DIRS} ? $ENV{CLAUDE_CODE_PLUGIN_DIRS} : "unset", "\n";"#;

#[test]
fn a_new_shell_loads_the_apps_plugin_beside_the_inherited_ones_only_while_the_switch_is_on() {
    let program = ShellProgram {
        path: "/usr/bin/perl".into(),
        arguments: vec!["-e".into(), REPORT.into()],
        home: "/".into(),
        open_file_limit: None,
        context: ShellContext {
            app: "taco-shells-dev".into(),
            port_file: "/dev-config/focus-port".into(),
            plugin_directory: OWN.into(),
        },
    };
    let (inputs, received) = mpsc::channel();
    let mut shells = PtyShells::new(program, inputs, Arc::new(Budgets::default()));

    // As when the app is opened from one of its own terminals, under a user's own plugin folder.
    unsafe { std::env::set_var("CLAUDE_CODE_PLUGIN_DIRS", format!("/theirs:{OWN}")) };
    let off = reported(&mut shells, &received, "off");
    assert_eq!(off, "/theirs");

    shells.set_agent_plugin(true);
    let on = reported(&mut shells, &received, "on");
    assert_eq!(on, format!("/theirs:{OWN}"));

    unsafe { std::env::set_var("CLAUDE_CODE_PLUGIN_DIRS", OWN) };
    shells.set_agent_plugin(false);
    let only_ours_off = reported(&mut shells, &received, "only-ours-off");
    assert_eq!(only_ours_off, "unset");
}

/// What a newly started shell reports for the variable.
fn reported(shells: &mut PtyShells, received: &Receiver<Input>, name: &str) -> String {
    let id = TerminalId(name.into());
    shells.start(&id, None, SIZE);
    let mut seen = String::new();
    while !seen.contains('\n') {
        let input = received.recv_timeout(PATIENCE);
        let Ok(input) = input else {
            panic!("nothing more from {name} after {seen:?}");
        };
        if let Input::Output { id: from, data } = input
            && from == id
        {
            seen.push_str(&String::from_utf8_lossy(&data));
        }
    }
    seen.trim().to_string()
}

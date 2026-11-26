use taco_protocol::{TerminalId, TerminalSize};

/// The shells the core drives: one per terminal, each in a pty of its own.
pub trait Shells {
    /// Starts a terminal's login shell in `directory`, or in the home directory when that is none or not a directory,
    /// and gives back the directory it started in: none when it could not start.
    fn start(
        &mut self,
        id: &TerminalId,
        directory: Option<&str>,
        size: TerminalSize,
    ) -> Option<String>;
    fn write(&mut self, id: &TerminalId, data: &[u8]);
    fn resize(&mut self, id: &TerminalId, size: TerminalSize);
    /// Ends the shell. Its exit is reported later, after its last output, like any other.
    fn close(&mut self, id: &TerminalId);
    /// The shell's own working directory, from the kernel.
    fn directory(&self, id: &TerminalId) -> Option<String>;
    /// Whether the shell holds its terminal's foreground: it is at its prompt rather than running a program.
    fn at_prompt(&self, id: &TerminalId) -> bool;
    /// The command lines in the terminal's foreground process group.
    fn foreground_commands(&self, id: &TerminalId) -> Vec<String>;
    fn shell_pid(&self, id: &TerminalId) -> Option<u32>;
}

pub const PANEL_TEXT: &str = concat!(
    "This terminal runs inside Taco Shells, which shows the user an Agent Activity panel beside it. The panel is yours to write: it is how the user follows your work without reading the terminal. Three tools write to it:",
    "\n\n",
    "- `now`: the one line at the top of the panel saying what you are on right now, under ten words. Set it when you start something and when your focus changes.",
    "\n",
    "- `note`: an entry in the user's log, one or two plain sentences. Write one after each meaningful step: a change made, a finding, a decision, a test run that passed or failed, something you need from the user. Not one per command.",
    "\n",
    "- `share`: put something in the panel's Shared list when the user should look at it: a document, an image, a pull request or an artifact you made yourself, or an artifact you read and judge important. Never sources or reference links.",
    "\n\n",
    "Write for a reader who glances at the panel between other things: high level, plain words, the outcome before the detail.",
    "\n\n",
    "When to write, so the panel is never empty while you work: set `now` before your first step. Write a `note` as each piece of work lands, and at the latest before your final reply. When you write something the user should read, a plan, a report or a diagram, `share` it.",
);

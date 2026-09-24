# Features

- In-app update detection
- In-app update process


- Upstream a PR to Clawd on Desk dropping the `editor` allowlist in
  `scheduleTerminalTabFocus`, so it posts `/focus-tab` to any terminal that
  serves it. The endpoint already exists here, so the attention hook stops being
  needed the day it merges.


# Nice-to-haves

- Taco Shells logo in the app (top of sidebar, other places too?)


# Bugfixes

- Currently, the latest hook addition breaks a bit because clicking the go to terminal link sometimes redirects you to the wrong terminal (it'll switch terminals to a wrong one). Not sure how to fix tbh, but even when not going through claude, on selection (enter press to send the response on a question) it'll switch
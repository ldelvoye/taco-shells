#!/bin/sh
# Plays a transcript into the terminal. The first line is the title the pane
# publishes; each line after it is a delay in seconds, a tab, and the text.
transcript="$1"
tab=$(printf '\t')

{
  IFS= read -r title
  printf '\033]0;%s\007' "$title"
  cleared=0
  while IFS="$tab" read -r delay text; do
    sleep "$delay"
    # Whatever the shell's start-up echoed is wiped just before the first line.
    if [ "$cleared" = 0 ]; then
      printf '\033[H\033[2J'
      cleared=1
    fi
    printf '%b\n' "$text"
  done
} < "$transcript"

# The pane closes with its shell, so the finished transcript stays on screen
# until the recording quits the app.
sleep 86400

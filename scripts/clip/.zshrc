# The recording points ZDOTDIR here, so every shell it opens plays the scene's
# next pane instead of a prompt: nothing is ever typed on camera. A pane is a
# transcript for replay.sh, or a program of its own.
count_file="$TACO_CLIP_STATE/count"
played=0
if [ -r "$count_file" ]; then
  played=$(cat "$count_file")
fi
next=$((played + 1))
printf '%s' "$next" > "$count_file"

pane="$TACO_CLIP_SCENE/$next"
if [ -f "$pane.txt" ]; then
  exec /bin/sh "$ZDOTDIR/replay.sh" "$pane.txt"
fi
exec node "$pane.mjs"

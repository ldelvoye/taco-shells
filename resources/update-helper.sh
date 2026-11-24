#!/bin/sh
# Arguments: <pid of the running app> <installed .app> <extracted .app>
set -u

pid=$1
installed=$2
extracted=$3

# Gives up after 30 seconds without touching anything: the app is still running then, and the banner offers the update again next launch.
attempts=0
while kill -0 "$pid" 2>/dev/null; do
  attempts=$((attempts + 1))
  if [ "$attempts" -ge 150 ]; then
    exit 0
  fi
  sleep 0.2
done

name=$(basename "$installed" .app)
stamp=$(date +%Y%m%d-%H%M%S)
trashed="$HOME/.Trash/$name $stamp.app"

# The app has quit by now, so every failure below reopens the installed app rather than leave nothing running.
if ! mv "$installed" "$trashed"; then
  open "$installed"
  exit 1
fi

if ! mv "$extracted" "$installed"; then
  if mv "$trashed" "$installed"; then
    open "$installed"
  fi
  exit 1
fi

if ! open "$installed"; then
  if mv "$installed" "$extracted" && mv "$trashed" "$installed"; then
    open "$installed"
  fi
  exit 1
fi

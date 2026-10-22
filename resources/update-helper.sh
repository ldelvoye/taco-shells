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

if ! mv "$installed" "$trashed"; then
  exit 1
fi

if ! mv "$extracted" "$installed"; then
  mv "$trashed" "$installed"
  exit 1
fi

if ! open "$installed"; then
  mv "$installed" "$extracted"
  mv "$trashed" "$installed"
  exit 1
fi

#!/bin/sh

. "$(dirname "$(readlink -f "$0")")/ancestors.sh"

payload=$(cat | tr -d '\n')

event=$(printf '%s' "$payload" | sed -n 's/.*"hook_event_name" *: *"\([^"]*\)".*/\1/p')
session_id=$(printf '%s' "$payload" | sed -n 's/.*"session_id" *: *"\([^"]*\)".*/\1/p')
# The match stops at the first quote, so only backslashes can reach the JSON body unescaped.
cwd=$(printf '%s' "$payload" | sed -n 's/.*"cwd" *: *"\([^"]*\)".*/\1/p' | sed 's/\\/\\\\/g')

if [ -z "$session_id" ]; then
  exit 0
fi

ports=$(taco_shells_ports)

if [ -z "$ports" ]; then
  exit 0
fi

pids=$(taco_shells_ancestor_pids)

if [ -z "$pids" ]; then
  exit 0
fi

if [ "$event" = "SessionStart" ]; then
  if [ -z "$cwd" ]; then
    exit 0
  fi
  route="/claude-session/start"
  body="{\"pids\":[$pids],\"sessionId\":\"$session_id\",\"cwd\":\"$cwd\"}"
elif [ "$event" = "SessionEnd" ]; then
  route="/claude-session/end"
  body="{\"pids\":[$pids],\"sessionId\":\"$session_id\"}"
else
  exit 0
fi

for port in $ports; do
  curl -s -m 1 -X POST -H 'Content-Type: application/json' \
    -d "$body" "http://127.0.0.1:$port$route" >/dev/null 2>&1
done

exit 0

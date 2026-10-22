#!/bin/sh
# Flags the pane whose shell is an ancestor of this hook's session as waiting for input.

. "$(dirname "$(readlink -f "$0")")/ancestors.sh"

ports=$(taco_shells_ports)

if [ -z "$ports" ]; then
  exit 0
fi

pids=$(taco_shells_ancestor_pids)

if [ -z "$pids" ]; then
  exit 0
fi

for port in $ports; do
  curl -s -m 1 -X POST -H 'Content-Type: application/json' \
    -d "{\"pids\":[$pids]}" "http://127.0.0.1:$port/attention" >/dev/null 2>&1
done

exit 0

#!/bin/sh
# Flags the pane whose shell is an ancestor of this hook's session as waiting for input.

if [ -n "$TACO_SHELLS_CONFIG_DIR" ]; then
  config_dirs="$TACO_SHELLS_CONFIG_DIR"
else
  config_dirs="$HOME/.taco-shells $HOME/.taco-shells-dev"
fi

ports=""
for config_dir in $config_dirs; do
  port_file="$config_dir/focus-port"
  if [ ! -r "$port_file" ]; then
    continue
  fi
  IFS= read -r port < "$port_file"
  if [ -z "$port" ]; then
    continue
  fi
  ports="$ports $port"
done

if [ -z "$ports" ]; then
  exit 0
fi

# Six ancestors reaches the pane's shell through the usual agent and wrapper
# layers. Too few and the pane is never flagged, with nothing said about it.
pids=$(ps -eo pid=,ppid= | awk -v start="$PPID" '
  {
    ppid[$1] = $2
  }
  END {
    pid = start
    out = ""
    count = 0
    while (pid != "" && pid != 0 && count < 6) {
      if (out == "") {
        out = pid
      } else {
        out = out "," pid
      }
      count++
      pid = ppid[pid]
    }
    print out
  }
')

if [ -z "$pids" ]; then
  exit 0
fi

for port in $ports; do
  curl -s -m 1 -X POST -H 'Content-Type: application/json' \
    -d "{\"pids\":[$pids]}" "http://127.0.0.1:$port/attention" >/dev/null 2>&1
done

exit 0

#!/bin/sh
# One native run of the shipped default on the claude CLI, on a scratch
# store and a scratch workspace; everything it made is kept.
#
#   run.sh start <name> <port> <models.json row> "<message>"
#   run.sh post  <name> <port> "<message>"      another message to the translator
#   run.sh watch <name> <port>                  one status line (engines, busy, messages)
#   run.sh capture <name>                       every engine's next prompt, as its model would get it
#   run.sh stop  <name>
#
# The store is /Volumes/Backup/dry-build/night-run/<name>/state, the
# workspace .../proj. No services, no MCP servers, no declared capabilities.
set -eu
cmd=$1; name=$2
W=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${NIGHT_BIN:-/Volumes/Backup/dry-build/night-target/debug}
R=/Volumes/Backup/dry-build/night-run/$name
export DRY_ROOT=$W DRY_STORE=md:$R/state DRY_CAPABILITIES=none DRY_DEFAULT=$W/default DRY_BIN=$BIN/dry
post() {
  python3 -c 'import json,sys; print(json.dumps({"text": sys.argv[1]}))' "$2" > "$R/body.json"
  curl -s -X POST "http://127.0.0.1:$1/api/engines/translator/messages" -H 'content-type: application/json' --data-binary @"$R/body.json"
  echo
}
case $cmd in
start)
  port=$3; row=$4; msg=$5
  [ -e "$R" ] && { echo "$R exists: pick another name"; exit 1; }
  mkdir -p "$R/proj"
  cd "$R/proj"
  "$BIN/dry" engine use translator "$row" > "$R/setup.log" 2>&1
  "$BIN/dry" prompt translator > "$R/prompt-translator-0.txt" 2>&1 || true
  nohup "$BIN/dry-server" --port "$port" > "$R/server.log" 2>&1 &
  echo $! > "$R/pid"
  for i in 1 2 3 4 5 6 7 8 9 10; do curl -s -o /dev/null "http://127.0.0.1:$port/api/status" && break; sleep 1; done
  text=$(printf '%s' "$msg" | sed "s#{proj}#$R/proj#g")
  printf '%s\n' "$text" > "$R/message.txt"
  date +%s > "$R/t0"
  post "$port" "$text"
  echo "started $name on $port (pid $(cat "$R/pid")): $R"
  ;;
post) post "$3" "$4" ;;
watch)
  curl -s "http://127.0.0.1:$3/api/status" | python3 -c '
import json,sys,time
s=json.load(sys.stdin)
print(time.strftime("%H:%M:%S"), " ".join("%s:%s%s/%d" % (e["id"], e.get("kind","?"), "*" if e.get("busy") else "", e.get("messages",0)) for e in s.get("engines",[])))'
  ;;
capture)
  out=$R/prompts/$(date +%H%M%S); mkdir -p "$out"
  cd "$R/proj"
  for d in "$R"/state/engines/*/; do id=$(basename "$d"); "$BIN/dry" prompt "$id" > "$out/$id.txt" 2>&1 || true; done
  echo "$out"
  ;;
stop) kill "$(cat "$R/pid")" 2>/dev/null || true; echo stopped ;;
esac

#!/bin/sh
# host.sh start <name> <port> <model-row> [context]   |  host.sh stop <name>
# Scratch dry-server: own root copy, own store, no capabilities, no MCP, no sessions.
set -eu
S=$(cd "$(dirname "$0")" && pwd)
W=/Users/kaush/Downloads/Dev/dry-learn
BIN=$W/target/debug
cmd=$1; name=$2; R=$S/$name
case $cmd in
start)
  port=$3; row=$4; ctx=${5:-}
  [ -e "$R" ] && { echo "$R exists"; exit 1; }
  mkdir -p "$R/root" "$R/proj"
  for d in routes themes scenes components default assets; do [ -e "$W/$d" ] && cp -R "$W/$d" "$R/root/$d"; done
  cp "$W/models.json" "$W/providers.json" "$R/root/"
  echo '{"tools": []}' > "$R/root/sessions.json"
  python3 - "$R/root/models.json" "$row" "$ctx" <<'PY'
import json,sys
p,row,ctx=sys.argv[1:4]
m=json.load(open(p)); m["default"]=row
m["models"]=[x for x in m["models"] if x["name"] in ("local","omlx")]
if ctx:
    for x in m["models"]: x["context"]=int(ctx)
json.dump(m,open(p,"w"),indent=2)
PY
  cd "$R/proj"
  export DRY_ROOT=$R/root DRY_STORE=md:$R/state DRY_CAPABILITIES=none DRY_DEFAULT=$R/root/default DRY_BIN=$BIN/dry
  nohup "$BIN/dry-server" --port "$port" > "$R/server.log" 2>&1 &
  echo $! > "$R/pid"
  for i in 1 2 3 4 5 6 7 8 9 10; do curl -s -o /dev/null "http://127.0.0.1:$port/api/status" && break; sleep 1; done
  echo "started $name on $port pid $(cat "$R/pid") root $R"
  ;;
prompt) cd "$R/proj"; DRY_ROOT=$R/root DRY_STORE=md:$R/state DRY_CAPABILITIES=none DRY_DEFAULT=$R/root/default "$BIN/dry" prompt "$3" ;;
restart) port=$3; kill "$(cat "$R/pid")" 2>/dev/null || true; sleep "${4:-1}"; cd "$R/proj"
  export DRY_ROOT=$R/root DRY_STORE=md:$R/state DRY_CAPABILITIES=none DRY_DEFAULT=$R/root/default DRY_BIN=$BIN/dry
  nohup "$BIN/dry-server" --port "$port" >> "$R/server.log" 2>&1 &
  echo $! > "$R/pid"; sleep 2; echo "restarted $name on $port" ;;
stop) kill "$(cat "$R/pid")" 2>/dev/null || true; echo stopped ;;
esac

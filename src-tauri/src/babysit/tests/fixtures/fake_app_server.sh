#!/bin/sh
# Deterministic JSONL App Server fixture used by Rust protocol tests.
# Arguments: mode events-file. It deliberately records method names only.
mode=${1:-complete}
events=${2:-}
turn_count=0

record() {
  if [ -n "$events" ]; then
    printf '%s\n' "$1" >> "$events"
  fi
}

while IFS= read -r line; do
  method=$(printf '%s' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
  request_id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  [ -n "$method" ] && record "$method"

  case "$method" in
    initialize)
      if [ "$mode" = "init-fail" ]; then
        printf '{"id":%s,"error":{"code":-32601,"message":"unsupported"}}\n' "$request_id"
        exit 17
      fi
      printf '{"id":%s,"result":{"userAgent":"fake"}}\n' "$request_id"
      ;;
    initialized)
      ;;
    thread/list)
      if [ "$mode" = "orphan" ]; then
        printf '{"id":%s,"result":{"data":[{"id":"orphan-thread","threadSource":"feature:narrant-babysit"}]}}\n' "$request_id"
      else
        printf '{"id":%s,"result":{"data":[]}}\n' "$request_id"
      fi
      ;;
    thread/start)
      if [ "$mode" = "start-fail" ]; then
        printf '{"id":%s,"error":{"code":-32602,"message":"invalid"}}\n' "$request_id"
        continue
      fi
      printf '{"id":%s,"result":{"thread":{"id":"thread-1"}}}\n' "$request_id"
      ;;
    turn/start)
      turn_count=$((turn_count + 1))
      printf '{"id":%s,"result":{"turn":{"id":"turn-%s","status":"inProgress"}}}\n' "$request_id" "$turn_count"
      if [ "$mode" = "hang" ]; then
        continue
      fi
      if [ "$mode" = "process-fail" ]; then
        exit 23
      fi
      if [ "$mode" = "repair-fail" ] || {
        [ "$mode" = "repair" ] && [ "$turn_count" -eq 1 ];
      }; then
        text='"{}"'
      else
        text='"{\"before\":{\"1\":\"before explanation\"},\"after\":{\"1\":\"after explanation\"}}"'
      fi
      printf '{"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-%s","item":{"type":"agentMessage","text":%s}}}\n' "$turn_count" "$text"
      printf '{"method":"turn/completed","params":{"threadId":"thread-1","turnId":"turn-%s","turn":{"id":"turn-%s","status":"completed","items":[]}}}\n' "$turn_count" "$turn_count"
      ;;
    turn/interrupt)
      printf '{"id":%s,"result":{}}\n' "$request_id"
      ;;
    thread/delete)
      printf '{"id":%s,"result":{}}\n' "$request_id"
      ;;
    *)
      if [ -n "$request_id" ]; then
        printf '{"id":%s,"result":{}}\n' "$request_id"
      fi
      ;;
  esac
done

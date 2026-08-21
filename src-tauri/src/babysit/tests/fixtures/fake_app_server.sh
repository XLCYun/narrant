#!/bin/sh
# Deterministic JSONL App Server fixture used by Rust protocol tests.
# Arguments: mode events-file. It records only protocol metadata, never prompts or source.
mode=${1:-complete}
events=${2:-}
turn_count=0

record() {
  if [ -n "$events" ]; then
    printf '%s\n' "$1" >> "$events"
  fi
}

contains() {
  printf '%s' "$line" | grep -F "$1" >/dev/null 2>&1
}

thread_start_response() {
  if [ "$mode" = "insecure-policy" ]; then
    printf '{"id":%s,"result":{"thread":{"id":"thread-1"},"cwd":"/tmp/narrant-review-workspace","sandbox":{"type":"readOnly","networkAccess":false}}}\n' "$request_id"
    return
  fi
  printf '{"id":%s,"result":{"thread":{"id":"thread-1"},"cwd":"/tmp/narrant-review-workspace","runtimeWorkspaceRoots":["/tmp/narrant-review-workspace"],"activePermissionProfile":{"id":"narrant-babysit-restricted-v1","extends":null},"sandbox":{"type":"readOnly","networkAccess":false},"instructionSources":[]}}\n' "$request_id"
}

while IFS= read -r line; do
  method=$(printf '%s' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
  request_id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  [ -n "$method" ] && record "$method"
  if contains 'src/example.ts' || contains 'const old = 1;'; then
    record "source-in-input"
  fi

  case "$method" in
    initialize)
      if [ "$mode" = "lost-thread-response" ] && [ -n "$events" ] && grep -q '^thread/start:lost$' "$events"; then
        old_home=$(cat "${events}.home" 2>/dev/null)
        if [ -n "$old_home" ] && [ ! -e "$old_home" ]; then
          record "lost-generation-home:removed"
        fi
        rm -f "${events}.home"
      fi
      if contains '"experimentalApi":true'; then
        record "initialize:experimental"
      else
        record "initialize:missing-experimental"
      fi
      if [ "$mode" = "init-fail" ]; then
        printf '{"id":%s,"error":{"code":-32601,"message":"unsupported"}}\n' "$request_id"
        exit 17
      fi
      if [ "$mode" = "init-no-agent" ]; then
        printf '{"id":%s,"result":{"codexHome":"/tmp/fake-codex"}}\n' "$request_id"
      else
        printf '{"id":%s,"result":{"userAgent":"fake","codexHome":"/tmp/fake-codex","platformFamily":"unix","platformOs":"linux"}}\n' "$request_id"
      fi
      ;;
    initialized)
      ;;
    config/read)
      if [ "$mode" = "auth-fail" ]; then
        printf '{"id":%s,"error":{"code":401,"message":"not logged in"}}\n' "$request_id"
        continue
      fi
      if [ "$mode" = "unknown-notification" ]; then
        printf '{"method":"future/notification","params":{"opaque":true}}\n'
      fi
      if [ "$mode" = "personal-model-instructions" ]; then
        printf '{"id":%s,"result":{"config":{"model_instructions_file":"/tmp/personal.md","mcp_servers":{}},"origins":{},"layers":null}}\n' "$request_id"
      else
        if [ -n "$CODEX_HOME" ] && [ -f "$CODEX_HOME/config.toml" ] && ! grep -q 'mcp_servers' "$CODEX_HOME/config.toml"; then
          record "codex-home:isolated"
        fi
        printf '{"id":%s,"result":{"config":{"mcp_servers":{}},"origins":{},"layers":null}}\n' "$request_id"
      fi
      ;;
    configRequirements/read)
      if [ "$mode" = "managed-instructions" ]; then
        printf '{"id":%s,"result":{"requirements":{"additionalDeveloperInstructions":"managed"}}}\n' "$request_id"
      else
        printf '{"id":%s,"result":{"requirements":null}}\n' "$request_id"
      fi
      ;;
    thread/list)
      if contains '"sourceKinds"'; then
        record "thread/list:source-filtered"
      else
        record "thread/list:unfiltered"
      fi
      if [ "$mode" = "thread-list-no-data" ]; then
        printf '{"id":%s,"result":{"nextCursor":null}}\n' "$request_id"
        continue
      fi
      if { [ "$mode" = "orphan" ] || { [ "$mode" = "lost-thread-response" ] && grep -q '^thread/start:lost$' "$events"; }; } && contains '"archived":false'; then
        printf '{"id":%s,"result":{"data":[{"id":"orphan-thread","source":"vscode","threadSource":"feature:narrant-babysit"},{"id":"other-thread","source":"appServer","threadSource":"feature:another-client"}],"nextCursor":null}}\n' "$request_id"
      else
        printf '{"id":%s,"result":{"data":[],"nextCursor":null}}\n' "$request_id"
      fi
      ;;
    thread/start)
      policy_ok=true
      for expected in \
        '"permissions":"narrant-babysit-restricted-v1"' \
        '"runtimeWorkspaceRoots":["/tmp/narrant-review-workspace"]' \
        '"default_permissions":"narrant-babysit-restricted-v1"' \
        '"mcp_servers":{}' \
        '":root":"deny"' \
        '":workspace_roots":{".":"read"}' \
        '"network":{"enabled":false}' \
        '"project_doc_max_bytes":0' \
        '"include_instructions":false' \
        '"mcp":{"enabled":false}' \
        '"apps":false' \
        '"hooks":false' \
        '"plugins":false' \
        '"personality":"none"'
      do
        if ! contains "$expected"; then
          policy_ok=false
        fi
      done
      if [ "$policy_ok" = true ]; then
        record "thread/start:restricted"
      else
        record "thread/start:insecure"
      fi
      if [ "$mode" = "rejected-permission" ]; then
        printf '{"id":%s,"error":{"code":-32602,"message":"invalid field permissions"}}\n' "$request_id"
        continue
      fi
      if [ "$mode" = "start-fail" ]; then
        printf '{"id":%s,"error":{"code":-32602,"message":"invalid"}}\n' "$request_id"
        continue
      fi
      if [ "$mode" = "lost-thread-response" ] && ! grep -q '^thread/start:lost$' "$events"; then
        touch "$CODEX_HOME/narrant-lost-generation-marker"
        printf '%s' "$CODEX_HOME" > "${events}.home"
        record "thread/start:lost"
        exit 29
      fi
      if [ "$mode" = "delayed-thread-start" ]; then
        sleep 0.2
      fi
      thread_start_response
      if [ "$mode" = "block-writer" ]; then
        sleep 10
      fi
      ;;
    turn/start)
      turn_thread_id=$(printf '%s' "$line" | sed -n 's/.*"threadId":"\([^"]*\)".*/\1/p')
      if [ "$turn_thread_id" = "__narrant_babysit_missing_thread__" ]; then
        record "turn/start:sentinel"
        if [ "$mode" = "rejected-output-schema" ]; then
          printf '{"id":%s,"error":{"code":-32602,"message":"invalid field outputSchema"}}\n' "$request_id"
        else
          printf '{"id":%s,"error":{"code":-32004,"message":"thread not found"}}\n' "$request_id"
        fi
        continue
      fi
      record "turn/start:real"
      record "agent-message"
      turn_count=$((turn_count + 1))
      if [ "$mode" = "rejected-output-schema" ]; then
        printf '{"id":%s,"error":{"code":-32602,"message":"invalid field outputSchema"}}\n' "$request_id"
        continue
      fi
      printf '{"id":%s,"result":{"turn":{"id":"turn-%s","status":"inProgress"}}}\n' "$request_id" "$turn_count"
      if [ "$mode" = "hang" ] || [ "$mode" = "delayed-thread-start" ] || [ "$mode" = "block-writer" ]; then
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
      if [ "$mode" = "stale" ]; then
        printf '{"method":"item/completed","params":{"threadId":"thread-1","turnId":"stale-turn","item":{"type":"agentMessage","text":"{}"}}}\n'
        printf '{"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"stale-turn","status":"completed","items":[]}}}\n'
      fi
      printf '{"method":"item/completed","params":{"threadId":"thread-1","turnId":"turn-%s","item":{"type":"agentMessage","text":%s}}}\n' "$turn_count" "$text"
      printf '{"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-%s","status":"completed","items":[]}}}\n' "$turn_count"
      ;;
    turn/interrupt)
      interrupt_thread_id=$(printf '%s' "$line" | sed -n 's/.*"threadId":"\([^"]*\)".*/\1/p')
      if [ "$interrupt_thread_id" = "__narrant_babysit_missing_thread__" ]; then
        record "turn/interrupt:sentinel"
        if [ "$mode" = "missing-interrupt" ]; then
          printf '{"id":%s,"error":{"code":-32601,"message":"method not found"}}\n' "$request_id"
        else
          printf '{"id":%s,"error":{"code":-32004,"message":"thread not found"}}\n' "$request_id"
        fi
        continue
      fi
      if [ "$mode" = "missing-interrupt" ]; then
        printf '{"id":%s,"error":{"code":-32601,"message":"method not found"}}\n' "$request_id"
        continue
      fi
      printf '{"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-%s","status":"interrupted","items":[]}}}\n' "$turn_count"
      record "emit:turn/completed:interrupted"
      printf '{"id":%s,"result":{}}\n' "$request_id"
      ;;
    thread/delete)
      thread_id=$(printf '%s' "$line" | sed -n 's/.*"threadId":"\([^"]*\)".*/\1/p')
      record "thread/delete:$thread_id"
      if [ "$mode" = "missing-delete" ]; then
        printf '{"id":%s,"error":{"code":-32601,"message":"method not found"}}\n' "$request_id"
        continue
      fi
      if [ "$mode" = "delayed-delete" ]; then
        sleep 0.2
      fi
      printf '{"id":%s,"result":{}}\n' "$request_id"
      ;;
    *)
      if [ -n "$request_id" ]; then
        printf '{"id":%s,"result":{}}\n' "$request_id"
      fi
      ;;
  esac
done

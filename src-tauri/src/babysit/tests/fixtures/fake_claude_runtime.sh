#!/bin/sh
set -eu

scenario=$1
events=$2
shift 2
printf '%s:%s\n' "$scenario" "$*" >> "$events"

case "$scenario:$1" in
  invalid:--version)
    exit 1
    ;;
  ready:--version|signed-out:--version|missing-flag:--version|malformed-auth:--version)
    printf '%s\n' '2.1.205 (Claude Code)'
    ;;
  ready:--help|signed-out:--help|malformed-auth:--help)
    printf '%s\n' '--output-format --json-schema --no-session-persistence --permission-mode --tools --disallowedTools'
    ;;
  missing-flag:--help)
    printf '%s\n' '--output-format --json-schema --no-session-persistence --permission-mode --tools'
    ;;
  ready:auth)
    printf '%s\n' '{"loggedIn":true,"authMethod":"oauth"}'
    ;;
  signed-out:auth)
    printf '%s\n' '{"loggedIn":false}'
    ;;
  malformed-auth:auth)
    printf '%s\n' '{"authMethod":"oauth"}'
    ;;
  *)
    exit 2
    ;;
esac

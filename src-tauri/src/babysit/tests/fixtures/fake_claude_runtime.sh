#!/bin/sh
set -eu

scenario=$1
events=$2
shift 2
printf '%s:%s\n' "$scenario" "$*" >> "$events"

case "$scenario:$1" in
  hang-version:--version|hang-help:--help|hang-auth:auth)
    sleep 30
    ;;
  invalid:--version)
    exit 1
    ;;
  ready:--version|signed-out:--version|missing-flag:--version|malformed-auth:--version|hang-help:--version|hang-auth:--version|initialization-failed:--version|incompatible:--version)
    printf '%s\n' '2.1.205 (Claude Code)'
    ;;
  ready:--help|signed-out:--help|malformed-auth:--help|hang-auth:--help|initialization-failed:--help)
    printf '%s\n' '--output-format --json-schema --no-session-persistence --permission-mode --tools --disallowedTools'
    ;;
  missing-flag:--help)
    printf '%s\n' '--output-format --json-schema --no-session-persistence --permission-mode --tools'
    ;;
  incompatible:--help)
    printf '%s\n' '--output-format'
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
  initialization-failed:auth)
    printf '%s\n' '{}'
    ;;
  *)
    exit 2
    ;;
esac

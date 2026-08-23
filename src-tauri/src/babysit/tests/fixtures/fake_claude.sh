#!/usr/bin/env bash
set -eu
scenario="$1"
events="$2"
shift 2
count_file="$events.count"
count=0
if [ -f "$count_file" ]; then count=$(cat "$count_file"); fi
count=$((count + 1))
printf '%s' "$count" > "$count_file"
{
  printf '%s\n' "round:$count"
  printf '%s\n' "$@"
  printf '%s\n' "prompt:start"
  cat
  printf '%s\n' "prompt:end"
} >> "$events"
case "$scenario" in
  success)
    printf '%s\n' '{"type":"result","structured_output":{"before":{"1":"旧值初始化"},"after":{"1":"新值初始化"}},"future_field":{"ignored":true}}'
    ;;
  malformed)
    printf '%s\n' 'not json'
    ;;
  repair)
    if [ "$count" -eq 1 ]; then
      printf '%s\n' '{"type":"result","structured_output":{"before":{},"after":{"1":"新值初始化"}}}'
    else
      printf '%s\n' '{"type":"result","structured_output":{"before":{"1":"旧值初始化"},"after":{"1":"新值初始化"}}}'
    fi
    ;;
  auth)
    printf '%s\n' '{"type":"result","is_error":true,"subtype":"not_authenticated"}'
    exit 1
    ;;
  incompatible)
    printf '%s\n' '{"type":"result","is_error":true,"subtype":"unsupported"}'
    exit 1
    ;;
  process_exit)
    printf '%s\n' "private-stdout-sentinel:$PWD"
    printf '%s\n' "private-stderr-sentinel:$PWD" >&2
    exit 23
    ;;
  hang)
    sleep 30 &
    printf '%s\n' "child:$!" >> "$events"
    wait
    ;;
esac

#!/bin/sh
set -eu

# 所有 linked worktree 共享 git common dir，因此锁文件也天然跨 worktree 共享。
common_dir=$(git rev-parse --path-format=absolute --git-common-dir)
lock_file="$common_dir/narrant-cargo-agent.lock"

if command -v lockf >/dev/null 2>&1; then
  exec lockf -k "$lock_file" cargo "$@"
fi

if command -v flock >/dev/null 2>&1; then
  exec flock "$lock_file" cargo "$@"
fi

echo "warning: lockf/flock unavailable; running Cargo without the repository lock" >&2
exec cargo "$@"

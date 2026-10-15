#!/bin/sh
set -eu

# Cargo Agent 和 Tauri 共用同一套 sccache 环境，同时保留 Cargo profile 的增量编译默认值。
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
rust_cache_runner="$script_dir/rust-cache.mjs"

# 所有 linked worktree 共享 git common dir，因此锁文件也天然跨 worktree 共享。
common_dir=$(git rev-parse --path-format=absolute --git-common-dir)
# 自托管 CI 的独立 clone 可显式复用本机开发锁，避免两套 checkout 同时冷编译。
lock_file=${NARRANT_CARGO_LOCK_FILE:-"$common_dir/narrant-cargo-agent.lock"}

if command -v lockf >/dev/null 2>&1; then
  exec lockf -k "$lock_file" node "$rust_cache_runner" run cargo "$@"
fi

if command -v flock >/dev/null 2>&1; then
  exec flock "$lock_file" node "$rust_cache_runner" run cargo "$@"
fi

echo "warning: lockf/flock unavailable; running Cargo without the repository lock" >&2
exec node "$rust_cache_runner" run cargo "$@"

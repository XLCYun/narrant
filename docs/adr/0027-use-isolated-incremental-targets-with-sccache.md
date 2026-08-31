---
status: accepted
---

# Use isolated incremental targets with sccache

Narrant keeps a separate Cargo `target` directory for every development worktree, leaves
Cargo incremental compilation enabled for both main and actively developed worktrees,
and uses a shared sccache rustc wrapper for reusable compilation results. Agent Cargo
commands remain serialized through the repository-wide lock because cache misses, build
scripts, and final linking still compete for the same machine resources. This combines
fast edits inside one worktree with safe reuse across worktrees without treating Cargo's
fingerprints or incremental state as a shared cache.

## Consequences

- Agents run Cargo through `scripts/cargo-agent.sh`; Tauri commands run through the
  repository's `pnpm tauri` script so both paths configure the same sccache environment.
- Development worktrees do not set `CARGO_INCREMENTAL=0`. Only CI or a measured one-shot
  build may disable incremental compilation deliberately.
- Divergent worktrees never share `CARGO_TARGET_DIR`. A worktree target is removed only
  after that worktree is no longer needed.
- The local sccache capacity defaults to 2 GiB instead of its upstream 10 GiB default.
  `pnpm rust:cache:status` reports its state and `pnpm rust:cache:clean` stops the local
  service and removes only sccache data, never Cargo targets.
- A missing sccache executable degrades to ordinary Cargo compilation with an explicit
  warning, so release and CI environments are not made dependent on a developer tool.

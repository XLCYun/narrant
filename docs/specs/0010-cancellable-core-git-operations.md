---
status: accepted
---

# Cancellable Core Git Operations

## Problem Statement

Tauri commands currently register cancellation tokens themselves and repeat a
start/run/finish protocol. Registration silently replaces duplicate operation IDs, a
cancel request that arrives before registration is lost, and cleanup depends on every
caller remembering to call `finish`. Core Git execution also represents timeout,
oversized output, spawn/read failures, non-zero exits, and termination failures as one
diagnostic string. Output limits are detected only after the child exits, and killing the
direct child does not guarantee that descendants have stopped or that pipes were reaped.

Narrant needs two adjacent deep modules: one for application-operation cancellation and
one for reliable Core Git subprocess execution. Review lifecycle remains responsible for
deciding which phases observe cancellation.

## Scope

- Introduce `CancellableOperations::run` and `CancellableOperations::cancel` as the only
  application-operation registration seam.
- Reject duplicate running operation IDs, retain an early cancellation request until the
  matching operation starts, and remove registrations automatically on every exit path.
- Rename the source-specific Tauri cancellation command and operation IDs to source-neutral
  `cancel_operation` and `operation-*` names.
- Introduce `CoreGitProcess::execute` with invocation classes for query, network, and
  workspace operations.
- Apply timeout and stdout/stderr limits centrally, drain both streams concurrently, and
  stop the process tree immediately when cancellation, timeout, or an output limit occurs.
- Return only after the child has been waited for and its output pipes have been reaped.
- Expose stable failure kinds for cancellation, timeout, output limit, spawn, exit, read,
  wait, and termination failures; diagnostic text must not drive application branching.
- Preserve a Core Git failure kind when callers add operation-specific context.

## Non-goals

- Do not move Review Snapshot staging, Workspace convergence, CAS activation, rollback,
  or runtime readers out of Review lifecycle.
- Do not make convergence cancellable after Review lifecycle starts changing a Workspace.
- Do not introduce another Git implementation or a trait for the single local executable.
- Do not interpret stderr as machine-readable state.

## Interfaces

```rust
CancellableOperations::run(operation_id, |context| async move { ... })
    -> Result<T, OperationError>
CancellableOperations::cancel(operation_id)
    -> Result<(), OperationError>

CoreGitProcess::execute(invocation, cancellation)
    -> Result<GitOutput, CoreGitFailure>
```

`OperationContext` carries a native `tokio_util::sync::CancellationToken` and is passed
through application operations as their first argument after `self`. `run` owns
registration, cancellation by operation ID, and release; callers do not receive
start/finish methods. Core Git receives the token directly. By convention, ordinary
downstream code only observes received tokens; operation creators and lifecycle managers
request cancellation. `clone()` shares a cancellation scope, while `child()` creates a
context whose token receives parent cancellation without propagating child cancellation
back to the parent. Production and tests use `new(token)` without source/signal wrappers
or test-only controllers. Async waits use `cancelled().await` without timer polling.
`background()` creates a root context without an external source, while
`without_cancellation()` derives a
non-observing convergence context from the current call. Cancelling an unknown ID records a
bounded short-lived early-cancellation entry so an abort racing command dispatch is not
lost. A second running operation with the same ID fails without affecting the first.

`GitInvocation` contains a repository path, a direct argument array, and a
`GitOperationClass`. Callers do not choose shell syntax, polling, pipe handling, timeout,
or termination policy.

## Review Lifecycle Integration

- Tauri commands wrap the complete application operation in `CancellableOperations::run`.
- Pull Request Selection resolution receives one `OperationContext` across its GitHub and
  Core Git preparation stages.
- Branch and Pull Request preparation observe that context.
- Review lifecycle performs one final cancellation check before initial materialization or
  refresh Workspace convergence, then derives `without_cancellation()`.
  Workspace creation, activation, and rollback therefore complete even if the original
  operation is cancelled during convergence.
- A failed or cancelled Core Git worktree creation compensates with a derived convergence
  context.
  Startup recovery removes managed Workspaces for unactivated Reviews before abandoning their
  candidate Snapshots, so the persisted ownership evidence remains available during cleanup.

## Testing Strategy

- Test `CancellableOperations` through `run` and `cancel`: running cancellation,
  cancellation before start, duplicate rejection, and reuse after automatic cleanup.
- Test `CoreGitProcess` with real temporary Git repositories: success, cancellation,
  timeout, output-limit classification, and contextual error-kind preservation.
- Test Review lifecycle only for the domain transition from cancellable preparation to
  derived cancellation-free convergence during initial materialization and refresh.
- Test Core Git worktree creation cancellation with a real repository and require the partial
  directory and linked-worktree registration to be gone before the operation returns.
- Test startup recovery with an unactivated Review that already owns a linked worktree.
- Test the typed Tauri transport for source-neutral operation IDs and cancellation command.

## Acceptance Criteria

1. No Tauri command manually pairs operation start and finish calls.
2. Duplicate running IDs are rejected and cannot replace another operation's token.
3. Cancellation requested before backend registration prevents the matching operation.
4. Completed, failed, and cancelled operations release their running registrations.
5. Frontend and Rust command inventories use `cancel_operation`; no
   `cancel_git_operation` or `git-operation-*` identifier remains.
6. Core Git failures expose stable kinds and context wrapping preserves those kinds.
7. Cancellation, timeout, and output overflow terminate and reap the Core Git process
   before returning.
8. Stdout and stderr are drained concurrently and either stream crossing its limit stops
   the command promptly.
9. Review preparation remains cancellable and Workspace convergence derives a context that
   does not observe later parent cancellation.
10. Rust formatting, Clippy, Rust tests, frontend tests, lint, typechecking, and production
    build pass.
11. Initial materialization cancellation cannot interrupt Workspace creation after convergence
    starts, and a failed worktree creation removes its partial directory and registration.
12. Startup recovery cleans Workspaces owned by unactivated Reviews before abandoning their
    candidate Snapshots.

## Related Decisions

- [ADR-0006: Use Core Git for repository operations](../adr/0006-use-core-git-for-repository-operations.md)
- [ADR-0012: Separate review refresh from analysis readiness](../adr/0012-separate-review-refresh-from-analysis-readiness.md)
- [ADR-0014: Use a typed Tauri transport and domain application adapters](../adr/0014-use-a-typed-tauri-transport-and-domain-application-adapters.md)
- [ADR-0017: Separate operation cancellation from Core Git execution](../adr/0017-separate-operation-cancellation-from-core-git-execution.md)

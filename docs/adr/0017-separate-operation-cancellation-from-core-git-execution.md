---
status: accepted
---

# Separate operation cancellation from Core Git execution

Narrant uses two adjacent modules for cancellable local work.

`CancellableOperations` owns application-operation identity, registration, early cancel
delivery, duplicate rejection, and automatic release. It is source-neutral and spans all
stages that a Tauri caller perceives as one operation.

`CoreGitProcess` owns one Core Git subprocess from direct argument construction through
concurrent output collection, limits, process-tree termination, waiting, and structured
failure reporting. It does not own application-operation IDs or decide when a domain
workflow stops honoring cancellation.

`OperationContext` is the strongly typed propagation object for one application call. It
carries a native `tokio_util::sync::CancellationToken`; it is neither a service locator
nor an extensions bag. App and Tauri state continue to own application-wide singletons.
`CancellableOperations` retains the token associated with each operation ID. Downstream
application code receives the context and Core Git receives the token directly.

The cancellation representation was simplified on 2026-09-14: source/signal wrappers
and separate test controllers were removed. Cancellation responsibility is a code
convention rather than a read-only type restriction:

- Ordinary downstream code observes a received token without calling `cancel()`.
- The operation creator or lifecycle manager cancels its managed scope, including
  stopping peer workers after a failure.
- `clone()` shares the same cancellation state. `OperationContext::child()` returns a
  context backed by `child_token()` when a scope needs independent cancellation;
  parent cancellation propagates down, while child cancellation does not affect parents
  or siblings.
- Production and tests construct contexts with `OperationContext::new(token)`.
- Async cancellation waits use `cancelled().await`. A native `drop_guard()` can request
  cancellation when an awaiting future is dropped; actual worker joining, process
  reaping and Workspace cleanup remain the owner's responsibility.

Review lifecycle makes the policy decision. Preparation observes the caller's operation
context. Workspace convergence derives `without_cancellation()` from that context so it
reaches activation or rollback once mutation begins. `background()` instead creates a
new root context for startup recovery and short non-registry entry points.

## Consequences

- Tauri command adapters contain one `run` call instead of a repeated start/finish
  protocol, and cancellation is no longer named after Git.
- An abort racing ahead of command registration is retained briefly and delivered when
  the operation starts; early-cancel storage is bounded and expires.
- Stable Core Git failure kinds, rather than stderr or formatted messages, drive control
  flow. Adding context preserves the original kind.
- Cancellation, timeout, and output overflow do not return while a child or inherited pipe
  remains unreaped.
- Core Git remains the sole local Git implementation and receives argument arrays without
  a shell.
- Pull Request Selection resolution and Review lifecycle keep their existing domain seams;
  application interfaces propagate `OperationContext`, while process mechanisms consume
  its native cancellation token directly.

The governing requirements are in
[Cancellable Core Git Operations](../specs/0010-cancellable-core-git-operations.md).

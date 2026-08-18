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
currently carries only a read-only `CancellationSignal`; it is neither a service locator
nor an extensions bag. App and Tauri state continue to own application-wide singletons.
`CancellableOperations` alone holds the private cancellation source, while downstream
application code receives the context and Core Git receives only its narrow signal.

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
  only its read-only signal.

The governing requirements are in
[Cancellable Core Git Operations](../specs/0010-cancellable-core-git-operations.md).

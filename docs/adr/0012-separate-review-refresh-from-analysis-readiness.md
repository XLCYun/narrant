---
status: accepted
---

# Separate Review refresh from analysis readiness

A Review refresh commits only after the new Review Snapshot, its File Changes, and the
Review Workspace all consistently represent the selected head revision. Language
servers and AI tooling initialize after that core transition and may enter a degraded
state without rolling the Review back; code comparison remains useful when optional
analysis tooling is unavailable, and tool configuration failures must not strand the
workspace and database on different revisions.

## Consequences

- Failures before the core transition completes leave the previous Snapshot current.
- Refresh preparation remains cancellable, but once Review Workspace transition begins,
  external cancellation cannot interrupt convergence: the lifecycle must either activate
  the new Snapshot or restore the previous Snapshot and Workspace.
- A persisted refresh candidate becomes part of Review Snapshot history only after
  successful activation. Rollback or startup recovery must keep failed candidates out of
  that history.
- After a crash, the last successfully activated current Snapshot is authoritative.
  Recovery restores that state rather than completing an unconfirmed refresh.
- After the core transition, LSP startup or indexing failure is reported as a capability
  failure while the new Snapshot remains current.
- Refresh recovery must reconcile SQLite state and the Narrant-owned worktree after a
  process crash because filesystem and database changes cannot share one transaction.

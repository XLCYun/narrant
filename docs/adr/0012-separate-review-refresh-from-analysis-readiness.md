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
- After the core transition, LSP startup or indexing failure is reported as a capability
  failure while the new Snapshot remains current.
- Refresh recovery must reconcile SQLite state and the Narrant-owned worktree after a
  process crash because filesystem and database changes cannot share one transaction.

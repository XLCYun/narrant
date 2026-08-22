---
status: accepted
---

# Retain open Reviews for the process lifetime

Narrant retains every Review opened during one application process, including its
Review Workspace, runtime content reader, and Review Reader navigation state, while
presenting only one Review at a time. Switching views or navigating to another Review
does not close the previous Review; returning to an already open Review reuses its
runtime entry and must not recreate its Workspace. This deliberately accepts unbounded
process-local worktree and memory growth because avoiding repeated Workspace creation is
more important than automatic eviction for the current product. A Snapshot refresh
replaces only that Review's materialized state, normal exit releases the complete Open
Review Set, and startup recovery safely removes leftovers from an interrupted process.

## Consequences

- The Review lifecycle owns an unbounded Open Review Set keyed by durable `reviewId`.
- There is exactly one Presented Review, but any number of other Reviews may remain open.
- No count, disk-usage, or idle-time eviction policy runs during the process.
- Selecting the Presented Review is a complete no-op. Presenting another already open
  Review updates its last-opened activity without materializing a Workspace.
- Reviews that are not presented perform no background update checks.
- Repository deletion, when implemented, must release owned runtime entries and
  Workspaces before deleting persistence records that prove Workspace ownership.
- The original switch-time cleanup and single-Workspace consequences of ADR-0008 are
  superseded; its disposability and exact-ownership safety requirements remain.

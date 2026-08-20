---
status: accepted
---

# Process-Lifetime Open Reviews

## Problem Statement

Switching from Review A to Review B currently closes A, removes its linked worktree,
and removes its runtime content reader. Returning to A therefore recreates and validates
the Review Workspace before the saved Snapshot can be presented. Cleanup also scans
persisted Reviews, so navigation cost can grow with Review history. Repeatedly selecting
the already presented Review follows the same reopen path even though the frontend
experience specification requires it to be a no-op.

Review navigation must reuse the complete runtime state of Reviews already opened in
the current application process. The goal is specifically to eliminate repeated Review
Workspace creation; this feature defines no elapsed-time service level.

## Scope

- Introduce a process-local Open Review Set keyed by durable `reviewId`.
- Keep exactly one Presented Review while retaining every previously opened Review.
- Retain each open Review's Review Workspace, runtime content reader, loaded File Change
  state, selected File Change, Review View, layout, and Reader navigation state.
- Reuse an existing runtime entry without validating or recreating its Workspace when
  the same current Snapshot remains bound to it.
- Treat navigation to Home, Repository Overview, and another Review as presentation
  changes rather than Review close operations.
- Keep existing explicit Snapshot refresh semantics and converge only the refreshed
  Review's Workspace and runtime reader.
- Release the entire Open Review Set during normal application exit.
- Continue safe cleanup of process leftovers during startup recovery.
- Preserve last-opened ordering: a real switch to another Review records activity;
  selecting the Presented Review is a complete no-op.

## Non-goals

- A maximum open-Review count, disk quota, LRU, TTL, or automatic eviction policy.
- A user-facing close-Review operation.
- Restoring open runtime or Reader state across application processes.
- Background update checks for Reviews that are not presented.
- Multiple simultaneously visible Review Readers or Review tabs.
- Repository deletion UI. A future delete operation must nevertheless clean up the
  Repository's open Reviews before deleting persistence records.
- A performance SLA beyond the behavioral requirement not to recreate a retained
  Workspace.

## Domain Semantics

- An Open Review is a durable Review with process-local runtime resources installed.
- The Open Review Set contains every Review opened during the current process and has no
  automatic size limit.
- The Presented Review is the one Open Review currently visible in the application.
- A Review owns exactly one Review Workspace for its current Snapshot. Historical
  Snapshots never own retained Workspaces.
- A successful Snapshot refresh discards the prior Snapshot's materialized runtime
  state and converges the same Review Workspace on the new Snapshot.
- Navigating away from a Review does not close it. Normal process exit releases all open
  Reviews; interrupted-process leftovers are verified and removed at next startup.

## Backend Lifecycle

### Present an unopened Review

1. Resolve or restore the target Review without changing the Presented Review.
2. Validate Repository availability and Workspace Trust.
3. Materialize and validate the target Review Workspace.
4. Install the target runtime content reader in the Open Review Set.
5. Mark the target Review opened and make it the Presented Review.
6. Do not remove or validate other Open Reviews or their Workspaces.

A target failure leaves the previous Presented Review and every Open Review intact.

### Present an already open Review

1. Find the runtime entry by durable `reviewId`.
2. Verify that its bound `snapshotId` equals the Review's current Snapshot identity
   already held by the application lifecycle.
3. Make it the Presented Review and update last-opened activity.
4. Do not call Core Git Workspace creation, transition, inspection, or cleanup.
5. Do not replace or remove another Open Review's runtime reader.

Selecting the already Presented Review returns success without persistence writes,
Core Git work, content reads, or frontend state changes.

### Refresh

Refresh remains explicit and applies to the Presented Review. Existing preparation,
candidate activation, rollback, cancellation, and crash-recovery invariants remain in
force. Once a candidate is activated, replace only that Review's runtime reader and
Reader state, and converge only that Review's Workspace on the new head. Other Open
Reviews remain readable and unchanged.

### Close and shutdown

Ordinary navigation does not call lifecycle close. The existing close operation may
remain as an internal idempotent primitive for future explicit close or Repository
deletion, but this feature exposes no user action that invokes it.

Shutdown clears all runtime readers and attempts exact-ownership cleanup for every Open
Review Workspace. A cleanup failure remains recoverable and must not delete Review
history. Startup recovery does not restore the previous Open Review Set; it verifies and
removes interrupted-process leftovers using the existing ownership proof.

### Concurrency

- Mutations of one Review remain serialized by `reviewId`.
- Presentation state changes are serialized separately from per-Review runtime entries.
- A failed target presentation cannot remove or corrupt the previous Presented Review.
- Refresh and shutdown cannot race into a partially activated Snapshot or an unowned
  Workspace cleanup.
- The lifecycle must not hold the global Open Review Set lock while running Core Git.

## Frontend Lifecycle

- Store Reader state by `reviewId`, not in a single replaceable Review slot.
- Reader state includes the presented Snapshot binding, loaded File Changes, selected
  path, current Overview/File Changes view, diff layout, and navigation position.
- Navigating away preserves the entry. Returning to it presents the retained state and
  requests only the backend presentation change.
- Sidebar selection of the Presented Review short-circuits before invoking the Review
  application adapter.
- Navigating from Review Reader to Repository Overview or Home changes the visible
  destination without calling backend close.
- Reviews that are not presented do not check for updates or refresh in the background.
- A successful Snapshot refresh resets only the refreshed Review's Snapshot-scoped
  Reader state.

## Compatibility

Existing Tauri command names and serialized Review Snapshot responses remain stable.
The `reopen_review` command changes lifecycle behavior from replacement to presentation
of an existing or newly materialized runtime entry. The existing `close_review` command
remains idempotent but is no longer invoked by normal frontend navigation.

## Testing Strategy

### Rust lifecycle tests

- Open A, open B, then present A again and prove no Workspace create, transition,
  inspection, or cleanup command runs for A or B.
- Prove A and B runtime readers remain installed after switching.
- Prove failed opening of C leaves B presented and A/B readable.
- Prove refreshing A replaces only A's reader and Workspace binding while B remains
  unchanged.
- Prove shutdown releases every runtime reader and attempts cleanup for every retained
  Workspace.
- Prove startup recovery removes safe leftovers without restoring an Open Review Set.
- Prove Core Git work occurs outside the global runtime lock.

### Frontend tests

- Selecting the Presented Review does not call the adapter or update last-opened state.
- A → B → A retains each Review's selected file, loaded content, Review View, diff
  layout, and navigation position.
- Returning to Repository Overview or Home does not call `close_review`.
- A real A → B or B → A switch invokes presentation and updates last-opened ordering.
- Refresh resets only the refreshed Review's Snapshot-scoped Reader state.
- Switching failure preserves the visible and retained states of all previously opened
  Reviews.

### Real application verification

- Open two Reviews in the Tauri application, switch A → B → A, and verify the second A
  presentation does not execute `git worktree add` and both linked worktrees remain.
- Verify Home and Repository Overview navigation leave opened Review Workspaces present.
- Verify normal application exit removes all retained linked worktrees.
- Verify wide and narrow navigation states through the application or Preview Harness.

## Acceptance Criteria

1. A → B → A during one process does not recreate, transition, inspect, or delete A's
   retained Review Workspace when A's current Snapshot has not changed.
2. Opening B does not remove A's runtime reader or Review Workspace.
3. The application retains complete Reader state independently for every Open Review.
4. Selecting the Presented Review performs no backend invocation, persistence write,
   content read, or visible state change.
5. Home, Repository Overview, and cross-Repository navigation do not close Reviews.
6. A real switch updates last-opened activity and changes only the Presented Review.
7. Inactive Reviews perform no background update checks.
8. Refresh changes only the refreshed Review and preserves existing convergence and
   rollback invariants.
9. Normal process exit releases every Open Review runtime and Workspace; startup safely
   cleans crash leftovers without restoring runtime state.
10. There is no automatic Open Review count, disk, or idle-time eviction.
11. Existing Workspace Trust and exact-ownership deletion safety remain enforced.
12. Rust formatting, Clippy, Rust tests, frontend lint, frontend tests, production build,
    and relevant real-render verification pass under supported toolchain versions.

## Related Decisions

- [ADR-0008: Treat Review Workspaces as disposable](../adr/0008-treat-review-workspaces-as-disposable.md)
- [ADR-0015: Preserve a multi-Review runtime representation](../adr/0015-preserve-multi-review-runtime-representation.md)
- [ADR-0021: Retain open Reviews for the process lifetime](../adr/0021-retain-open-reviews-for-the-process-lifetime.md)

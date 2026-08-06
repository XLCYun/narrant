---
status: accepted
---

# Review Lifecycle Backend

## Problem Statement

The Tauri command module currently coordinates Review persistence, Core Git work,
Review Workspace transitions, optimistic current-Snapshot activation, rollback,
cancellation, and runtime readers. Branch Selection and Pull Request Selection refresh
commands repeat the same ordering protocol, while tests reproduce that protocol by
calling the Repository Catalog and Core Git adapter directly. The command seam therefore
leaks lifecycle knowledge and is not the test surface for the behavior it exposes.

Narrant needs one deep Review lifecycle module whose interface expresses Review intent
and whose implementation owns the complete transition. Tauri remains a transport
adapter and existing frontend command contracts remain stable during this refactor.

## Scope

- Introduce a Rust Review lifecycle module that owns opening, switching, update checks,
  refresh, File Change reads, closing, and startup recovery.
- Keep the runtime representation keyed by durable `reviewId`, even while the first
  release continues to present only one active Review.
- Move Workspace Trust gates, Repository Entry availability checks, Review Selection
  validation, Review Workspace ordering, current-Snapshot activation, rollback, runtime
  reader replacement, and cleanup outcomes behind the lifecycle seam.
- Keep Tauri types, serialized command responses, command names, operation IDs, and user
  error messages outside the lifecycle module.
- Keep SQLite and Core Git as concrete local-substitutable dependencies. Tests use a
  temporary SQLite catalog and real temporary Git repositories rather than new mock
  traits.
- Make Review Refresh Candidate persistence distinct from Review Snapshot history.
- Replace tests that manually reproduce lifecycle ordering with tests through the new
  lifecycle interface. Retain focused catalog migration, Core Git machine-format, and
  Review Workspace ownership tests.

## Non-goals

- Do not change the frontend-visible Tauri command contract in this work.
- Do not move GitHub authentication or Pull Request discovery into the lifecycle module.
- Do not redesign Pull Request Selection resolution; the transport adapter may continue
  to provide a current Pull Request observation during the first migration.
- Do not add a Core Git, Repository Catalog, clock, filesystem, or runtime-state trait
  without a second real adapter.
- Do not enable multiple simultaneously presented Reviews yet.
- Do not add analysis tooling, Review deletion, historical Snapshot browsing, or new UI.

## Lifecycle Interface

The module exposes domain operations rather than source-specific Tauri commands:

```rust
present(request, cancellation)
check_update(review_id, observation, cancellation)
refresh(review_id, evidence, cancellation)
read_file_change(snapshot_binding, path, cancellation)
close(review_id)
recover()
```

The exact Rust types may evolve during implementation, but the following interface
rules are normative:

- Inputs use Review, Review Selection, Repository Entry, and Snapshot identities rather
  than Tauri state or serialized response models.
- Branch and Pull Request differences are represented by typed domain variants.
- Results and failures are typed lifecycle outcomes. The Tauri adapter maps them to the
  existing serialized responses and user-facing error strings.
- Callers cannot stage a candidate, activate a Snapshot, replace a runtime reader, or
  manipulate a Review Workspace as independent lifecycle steps.

## Invariants and Ordering

### Opening and switching

1. Validate the Repository Entry and Workspace Trust before fetch or checkout.
2. Resolve or restore the target Review without changing the currently readable Review.
3. Materialize and validate the target Review Workspace.
4. Install the target runtime reader only after the target is internally consistent.
5. Enforce the current single-active-Review presentation policy while retaining the
   multi-Review runtime representation required by ADR-0015.
6. A target failure leaves the previously readable Review intact.

### Update checks

- Resolve the moving Review Selection and report current and available revisions.
- Do not create a Review Refresh Candidate.
- Do not change the current Review Snapshot, Review Workspace, or runtime reader.

### Refresh preparation

The preparation phase remains cooperatively cancellable:

1. Read and bind the current Review Snapshot.
2. Validate Review Selection identity and the user's confirmed revisions.
3. Fetch or resolve the selected refs.
4. Calculate the current three-dot Change Comparison and File Changes.
5. Persist a Review Refresh Candidate that is excluded from Review history.

Cancellation or failure in this phase keeps the previous current Review Snapshot and
Review Workspace authoritative. The failed candidate is deleted or marked abandoned so
it cannot appear in Review history.

### Refresh convergence

Once Review Workspace mutation begins, external cancellation cannot interrupt
convergence:

1. Validate exact Review Workspace ownership.
2. Transition the Review Workspace to the candidate head revision.
3. Activate the candidate with a compare-and-swap against the expected current Snapshot.
4. Replace the runtime reader with one bound to the activated Snapshot.
5. Make the activated candidate visible as a Review Snapshot in history.

If activation fails, restore the previous Review Workspace with an internal
non-cancellable operation and preserve the previous runtime reader. A result may report
success only when catalog current state, the Review Workspace, and the runtime reader
refer to the same Review Snapshot.

### File Change reads

- Bind each read to both `reviewId` and immutable `snapshotId`.
- Reject a stale binding after refresh rather than reading content from another
  Snapshot.
- Closing a Review prevents new reads while already-held in-process references may
  finish safely.

### Closing

- Logical close removes the runtime reader before disposable Workspace cleanup.
- Review Workspace cleanup failure is a typed deferred-cleanup outcome; it never
  reopens the Review or deletes saved Review history.
- Close remains idempotent.

### Startup recovery

- The last successfully activated current Review Snapshot is authoritative.
- Recovery never completes an unconfirmed refresh after a crash.
- Unactivated candidates remain outside Review history and are abandoned or deleted.
- Leftover Review Workspaces are removed only after the exact catalog record, Git common
  directory, registered worktree path, and Narrant-managed path prove ownership.
- One failed recovery target does not block other safe targets.
- Recovery never performs broad worktree pruning.

## Persistence

The Review Catalog must distinguish an activated Review Snapshot from a Review Refresh
Candidate. Historical Snapshot queries return only candidates that were successfully
activated at least once. A schema migration must preserve every existing Snapshot as
activated because older releases persisted only successful historical/current
Snapshots, except for incomplete candidates that the migration can prove were never
current.

Candidate activation and the Review current pointer update occur in one SQLite
transaction. Candidate abandonment must not change the previous current pointer.

## Cancellation and Concurrency

- The transport cancellation registry maps `operationId` to a cancellation token outside
  the lifecycle seam.
- The lifecycle module decides where that token is observed and where convergence uses
  an internal non-cancellable token.
- Operations that mutate one Review are serialized by `reviewId`; future different
  Reviews may execute independently without changing the runtime representation.
- Duplicate refresh attempts for one Review fail deterministically without replacing an
  existing cancellation path.

## Errors and Outcomes

The lifecycle module distinguishes at least:

- Repository unavailable and Workspace Trust required;
- Review missing, closed, or busy;
- stale current Snapshot and Selection identity mismatch;
- cancellation during preparation;
- catalog and Core Git failures;
- Workspace ownership or transition failure;
- activation failure with successful rollback;
- activation and rollback failure requiring recovery;
- logical close with deferred Workspace cleanup.

Tauri adapters own localization and compatibility with the existing `Result<_, String>`
command contract.

## Testing Strategy

- Exercise lifecycle behavior through the same public Rust interface used by Tauri
  adapters.
- Use temporary SQLite catalogs and real temporary Git repositories.
- Cover Branch Selection and Pull Request Selection with the same convergence assertions.
- Cover preparation cancellation, concurrent refresh rejection, stale activation,
  successful refresh, activation rollback, failed switch preserving the previous Review,
  stale File Change reads, close cleanup failure, and crash recovery.
- Delete or rewrite orchestration tests that call candidate persistence, Workspace
  transition, and activation as an externally ordered protocol.
- Retain focused low-level tests for Core Git cancellation and limits, machine-format
  parsing, Workspace ownership validation, catalog migrations, and GitHub metadata-only
  discovery.

## Acceptance Criteria

1. Existing frontend Tauri command names, arguments, and serialized results remain
   compatible.
2. Tauri command functions no longer coordinate Review Workspace transition, candidate
   activation, rollback, or runtime reader replacement directly.
3. Branch and Pull Request refresh share one lifecycle convergence implementation.
4. A failed or cancelled preparation leaves the previous Review Snapshot and Review
   Workspace current and readable.
5. Once Workspace transition begins, refresh finishes activation or rollback despite a
   late external cancellation.
6. Review history excludes every candidate that was never successfully activated.
7. Startup recovery restores the last activated current Review Snapshot and never
   activates an unconfirmed candidate.
8. A failed Review switch preserves the previously readable Review.
9. Closing remains logically successful when exact Workspace cleanup must be deferred.
10. Runtime Review state remains keyed by durable `reviewId`.
11. Lifecycle tests use temporary SQLite and real Core Git through the lifecycle
    interface; no new single-adapter traits are introduced.
12. Rust formatting, Clippy, and all Rust tests pass. Relevant frontend transport tests
    and the production frontend build pass under a supported Node version.

## Related Decisions

- [ADR-0006: Use Core Git for repository operations](../adr/0006-use-core-git-for-repository-operations.md)
- [ADR-0007: Require Workspace Trust](../adr/0007-require-workspace-trust.md)
- [ADR-0008: Treat Review Workspaces as disposable](../adr/0008-treat-review-workspaces-as-disposable.md)
- [ADR-0011: Store Review catalog in SQLite](../adr/0011-store-review-catalog-in-sqlite.md)
- [ADR-0012: Separate Review refresh from analysis readiness](../adr/0012-separate-review-refresh-from-analysis-readiness.md)
- [ADR-0013: Unify Reviews on local repositories](../adr/0013-unify-reviews-on-local-repositories.md)
- [ADR-0014: Use a typed Tauri transport and domain application adapters](../adr/0014-use-a-typed-tauri-transport-and-domain-application-adapters.md)
- [ADR-0015: Preserve a multi-Review runtime representation](../adr/0015-preserve-multi-review-runtime-representation.md)

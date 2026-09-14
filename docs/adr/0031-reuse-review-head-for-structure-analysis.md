# Reuse Review head for Structure Changes analysis

Status: accepted

Date: 2026-09-12

## Context

Structure Changes compares two complete repository states using the existing language
analyzers. An open Review already owns a detached head checkout at its Snapshot's
head revision. Creating another head checkout would duplicate sources and build work.
Returning the existing path alone does not protect it against refresh or close.

## Decision

Reuse the existing Review Workspace head through
`ReviewLifecycle::run_in_workspace(operation, review_id, snapshot_id, work).await`.
The synchronous callback borrows a `SnapshotWorkspace` containing the fixed
`ChangeComparison`, repository/common-directory/head/before paths, and an already
bound `OperationContext`. It must join all child work before returning.
Its result cannot borrow from the workspace. Prepare only
an additional `reviews/<review-id>/before` worktree at the saved comparison-base commit.
The lease is a private Implementation of Review Lifecycle. The execution Interface
starts the actual blocking worker and moves the entire lease into it. It serializes
actual workers for a Review and blocks head mutation. The caller no longer acquires
a lease, attaches revocation flags, installs an abandonment guard, or arranges which
task owns directory protection. Head ownership and revision checks run before work
and before a successful result is returned.

Each check verifies the Repository's Git common directory, the canonical registered
worktree path and its Snapshot HEAD, then compares tracked contents against HEAD with
`git diff --quiet HEAD --`. Path and HEAD are checked against the same fresh
`git worktree list --porcelain -z` result; no worktree list is cached between checks.
These are checks of the state observed before work and after successful work. They
exclude untracked files and cannot detect an external edit that is restored between
checks. The lease serializes application-managed Workspace changes; it does not provide
filesystem snapshot isolation against external processes. The final check remains part
of the success contract and rejects a changed HEAD or tracked contents that remain
modified at that point.

Refresh, close and shutdown revoke analysis, wait for its actual workers to release the
lease, then change or remove head. The worker owns cleanup even if its asynchronous
caller disappears; cancellation reaches the language-server process and its children.

`OperationContext::child()` returns a context carrying a native `tokio-util` child
cancellation token, without a separate source/signal pair. Cancellation is requested
by the operation creator or lifecycle manager; ordinary downstream code observes the
received token by convention. Parent cancellation directly
wakes descendants; child cancellation cannot cancel parents or siblings. Workspace
revocation and abandonment stop the workspace child, and a failed analysis stops its
own child group. Abandonment uses the token's native `drop_guard()`; the actual worker
still owns the lease until work and cleanup finish. No timer combines resource flags.
The admission `changing` state
and recorded errors remain independent of cancellation.

Within one leased analysis, before checkout/preparation and analysis run concurrently
with analysis of the existing head. The lease serializes separate analysis attempts,
not their two sides. Failure on one side revokes its peer, and the lease remains held
until both workers have exited. This parallelization was added on 2026-09-14 without
changing checkout ownership.

Structure Changes retains two-side concurrency and before preparation/cleanup inside
its synchronous callback; Review Lifecycle knows neither Code Graph nor analyzer
options. Analysis errors preserve explicit cancellation instead of deriving it from
a later signal. The first real failure takes precedence over peer/user cancellation.
Analyzer panics are caught while the before checkout remains alive, followed by
explicit, uncancellable cleanup. Cleanup failures are reported with the original
failure or panic rather than only logged during Drop. All workers finish before the
callback returns, including failure and cleanup paths.

Before uses a fixed sibling path because worker lifetimes are serialized, including
cleanup. Existing persisted Review ownership plus exact Git common-directory and
worktree registration checks suffice to recover it after a process crash. Successful
graphs remain in process memory under Snapshot identity; checkouts are not a graph cache.

## Consequences

Only one extra source checkout is needed. Language analyzers still receive a repository
root and relative paths, without knowing Review lifecycle or Git preparation details.
Review mutations may briefly wait for cancellation and cleanup. A bare workspace-path
lookup is unsuitable for consumers that require coordination with Workspace mutations
and validation of the selected Snapshot across a long operation.
Other long-running consumers can adopt this protection at their own integration seam;
this change does not redefine Babysit's current lifecycle.

See [the Structure Changes specification](../specs/0020-structure-changes-review-view.md).

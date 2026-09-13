# Reuse Review head for Structure Changes analysis

Status: accepted

Date: 2026-09-12

## Context

Structure Changes compares two complete repository states using the existing language
analyzers. An open Review already owns a detached head checkout at its Snapshot's
head revision. Creating another head checkout would duplicate sources and build work.
Returning the existing path alone does not protect it against refresh or close.

## Decision

Reuse the existing Review Workspace head under an owned Snapshot lease. Prepare only
an additional `reviews/<review-id>/before` worktree at the saved comparison-base commit.
The lease serializes actual analysis workers for a Review and blocks head mutation.
Refresh, close and shutdown revoke analysis, wait for its actual workers to release the
lease, then change or remove head. The worker owns cleanup even if its asynchronous
caller disappears; cancellation reaches the language-server process and its children.

Within one leased analysis, before checkout/preparation and analysis run concurrently
with analysis of the existing head. The lease serializes separate analysis attempts,
not their two sides. Failure on one side revokes its peer, and the lease remains held
until both workers have exited. This parallelization was added on 2026-09-14 without
changing checkout ownership.

Before uses a fixed sibling path because worker lifetimes are serialized, including
cleanup. Existing persisted Review ownership plus exact Git common-directory and
worktree registration checks suffice to recover it after a process crash. Successful
graphs remain in process memory under Snapshot identity; checkouts are not a graph cache.

## Consequences

Only one extra source checkout is needed. Language analyzers still receive a repository
root and relative paths, without knowing Review lifecycle or Git preparation details.
Review mutations may briefly wait for cancellation and cleanup. A bare workspace-path
lookup is unsuitable for consumers that require immutable source across a long operation.
Other long-running consumers can adopt this protection at their own integration seam;
this change does not redefine Babysit's current lifecycle.

See [the Structure Changes specification](../specs/0020-structure-changes-review-view.md).

---
status: accepted
---

# Pull Request Selection Resolution

## Problem Statement

Pull Request discovery currently returns both stable selection identity and an ephemeral
GitHub observation. The frontend resolves a selection by repeating full repository
discovery, caches that observation, and sends it back through the Tauri transport when a
Review is opened, checked, or refreshed. Review lifecycle then performs remote matching,
special-ref fetches, revision validation, and comparison calculation itself.

This makes callers responsible for keeping moving Pull Request metadata fresh and splits
one resolution protocol across discovery, the frontend adapter, Tauri commands, and the
Review lifecycle module. Narrant needs one deep Pull Request Selection resolution module
that accepts only durable identity and returns one locally verified immutable comparison.

## Scope

- Introduce a Rust Pull Request Selection resolver whose public interface accepts a local
  repository path, GitHub repository identity, pull-request number, and cancellation.
- Observe the selected Pull Request through GitHub's single-Pull-Request endpoint.
- Match every configured Git remote representing the selected base repository and try
  matching remotes deterministically until the required objects can be fetched.
- Fetch the current base branch and `refs/pull/<number>/head` without creating a local
  branch, remote-tracking ref, tag, or `FETCH_HEAD` entry.
- Verify GitHub's exact base and head revisions locally, compute the merge base, and build
  the three-dot Change Comparison through Core Git.
- Return the immutable local Snapshot together with synchronized Pull Request metadata.
- Make Review lifecycle resolve its durable Pull Request Selection internally for open,
  update-check, and refresh operations.
- Reduce frontend and Tauri command inputs to stable selection identity or confirmed
  revision evidence.

## Non-goals

- Do not use GitHub's Files changed endpoint or GitHub's synthetic merge ref.
- Do not persist `refs/pull/*` or modify the user's configured fetch refspecs.
- Do not change Pull Request discovery grouping or the visible selection UI.
- Do not move Snapshot staging, Workspace transitions, activation, rollback, or runtime
  readers out of Review lifecycle.
- Do not add a second Git implementation or traits for local Git and SQLite dependencies.

## Resolver Interface

```rust
resolve(repository_path, pull_request_selection, cancellation)
    -> ResolvedPullRequestSelection
```

The interface uses only durable Pull Request Selection identity. Its result contains the
current collaboration metadata and a locally verified immutable Snapshot. GitHub wire
models, remote names, special refs, fetch ordering, object validation, and comparison
calculation remain implementation details.

## Review Lifecycle Integration

- Opening a new Pull Request Review resolves `{ repositoryEntryId, owner, repository,
  number }`; reopening a saved Review uses its current Snapshot without GitHub access.
- Checking for updates takes only `reviewId`, resolves the Review's durable Selection, and
  never creates a candidate or mutates the current Workspace.
- Refreshing takes `reviewId` plus the exact current and available revisions the user
  confirmed. Review lifecycle resolves again and rejects the refresh if either the
  current Snapshot or moving Pull Request Selection changed after confirmation.
- Review lifecycle remains the sole owner of candidate persistence, Workspace convergence,
  CAS activation, rollback, and runtime-reader replacement.

## Errors

Resolution distinguishes invalid selection identity, GitHub observation failure, missing
matching remotes, cancellation, failure across all matching remotes, and exact-revision
validation failure. The Tauri adapter keeps mapping typed failures into the existing
`Result<_, String>` transport convention.

## Testing Strategy

- Test the resolver through its public interface with a local HTTP server and real
  temporary Git repositories/remotes.
- Verify that stable identity produces synchronized metadata and a three-dot local
  Snapshot, and that resolution falls back across multiple matching remotes.
- Test Review lifecycle through its public interface to prove Pull Request open, update
  check, stale confirmation rejection, and refresh convergence.
- Replace frontend tests for rediscovery/cached observations with transport-contract tests
  proving only stable identity and confirmed revisions cross the Tauri seam.

## Acceptance Criteria

1. Frontend Review code no longer resolves or caches Pull Request observations.
2. `open_pull_request_review` accepts only Repository Entry identity and Pull Request
   Selection identity.
3. `check_pull_request_review_update` accepts only `reviewId` and operation identity.
4. `refresh_pull_request_review` accepts only `reviewId`, operation identity, and confirmed
   current/available revisions.
5. Review lifecycle public inputs contain no GitHub discovery DTO or observation.
6. The resolver fetches and validates the exact GitHub revisions before returning.
7. Failure of one matching remote does not prevent resolution through another matching
   remote; failure diagnostics retain all attempted remote names.
8. Update checks do not mutate current Snapshot, Review Workspace, or runtime reader.
9. A Pull Request change after confirmation is rejected before candidate staging.
10. Pull Request and Branch refresh continue to share lifecycle convergence.
11. Rust formatting, Clippy, Rust tests, frontend tests, lint, typechecking, and production
    build pass.

## Related Decisions

- [ADR-0006: Use Core Git for repository operations](../adr/0006-use-core-git-for-repository-operations.md)
- [ADR-0013: Unify Reviews on local repositories](../adr/0013-unify-reviews-on-local-repositories.md)
- [ADR-0014: Use a typed Tauri transport and domain application adapters](../adr/0014-use-a-typed-tauri-transport-and-domain-application-adapters.md)
- [ADR-0016: Resolve Pull Request Selections behind one seam](../adr/0016-resolve-pull-request-selections-behind-one-seam.md)

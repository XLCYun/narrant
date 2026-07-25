---
status: accepted
---

# Unify Reviews on local repositories

Every Review belongs to a specific locally cloned Repository Entry and obtains code,
File Changes, and its Review Workspace through Core Git. A Pull Request is a durable
Review Selection and source of GitHub collaboration metadata, not a second code-content
adapter; this replaces the former Local/GitHub snapshot split and the runtime
Review Session identity with one persisted Review model keyed by `reviewId`.

## Consequences

- Users select a local clone before creating or opening any Review.
- Branch and Pull Request Selections converge on current base and head commits before
  Snapshot creation.
- GitHub no longer supplies File Changes, trees, blobs, or file contents.
- A durable Review contains successive immutable Review Snapshots and owns a temporary
  local Review Workspace only while open.
- The command and presentation seam uses `reviewId` and Snapshot identity rather than a
  source-specific union or `sessionId`.
- ADR-0002 and ADR-0005 are superseded. The governing requirements are in
  [Local Repository Reviews](../specs/0003-local-repository-reviews.md).

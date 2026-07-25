---
status: accepted
---

# Treat Review Workspaces as disposable

Each open Review owns a Narrant-managed workspace at
`reviews/<review-id>/repo` whose checkout is derived entirely from the head revision of
the Review's current snapshot. The workspace is not a place for user-owned data:
Narrant may forcibly discard tracked, untracked, and ignored changes when advancing or
rebuilding it, which keeps LSP and AI analysis aligned with the exact head commit and
avoids making external tool side effects part of the Review model.

## Consequences

- Narrant may use forced checkout, hard reset, and clean operations inside a verified
  Review Workspace without asking whether its contents should be preserved.
- Every destructive operation must first prove that the target path is the expected
  Narrant-managed directory and the registered linked worktree for that exact Review;
  unresolved variables, broad paths, and unverified directories are never valid
  targets.
- Durable reports and other future Review artifacts live outside `repo/`.
- LSP indexes and other reusable tool state should live outside `repo/` so rebuilding
  the checkout does not erase them unnecessarily.
- The UI must not present the Review Workspace as a general-purpose editable checkout.
- Workspaces are created lazily and removed when the Review closes, the user switches
  Reviews, or the application exits; SQLite Review history remains.
- The first release keeps at most one Review Workspace active and does not retain an
  unbounded checkout cache for historical Reviews.
- On startup Narrant automatically cleans up worktrees left by an interrupted prior
  run, but only after the database record, Git common directory, registered worktree
  path, and Narrant-managed absolute path all identify the same exact workspace. It
  never runs a broad prune over user- or third-party-managed worktrees.

---
status: accepted
---

# Treat Review Workspaces as disposable

The process-lifetime retention policy in
[ADR-0021](0021-retain-open-reviews-for-the-process-lifetime.md) supersedes this
decision's original single-Workspace and switch-time cleanup rules. Review Workspaces
remain disposable, Narrant-owned checkouts subject to the safety rules below.

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
- Workspaces are created lazily and retained while their Review belongs to the current
  process's Open Review Set. Application exit removes them; SQLite Review history remains.
- Closing a Review releases its runtime reader even when workspace removal fails. The
  cleanup failure remains reportable and recoverable, but does not reopen the Review.
- A Workspace belongs only to its Review's current Snapshot. Historical Snapshots do
  not retain separate worktrees.
- On startup Narrant automatically cleans up worktrees left by an interrupted prior
  run, but only after the database record, Git common directory, registered worktree
  path, and Narrant-managed absolute path all identify the same exact workspace. It
  never runs a broad prune over user- or third-party-managed worktrees.

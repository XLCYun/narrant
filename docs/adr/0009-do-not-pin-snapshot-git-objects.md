---
status: accepted
---

# Do not pin Snapshot Git objects

Narrant stores the exact base, comparison-base, and head object IDs of every Review
Snapshot but does not create persistent refs in the user's repository to keep those
objects reachable. Avoiding a private `refs/narrant/*` namespace keeps repository
intrusion limited to the accepted linked-worktree metadata, at the cost that historical
Snapshot content may become unavailable after branch deletion, force-push, pruning, or
garbage collection.

## Consequences

- Snapshot identity, File Change metadata, findings, and review history may remain in
  SQLite after the corresponding Git objects disappear.
- Narrant does not persist or proactively maintain an availability flag. If the user
  later asks to materialize historical content whose objects are gone, the attempted
  Core Git operation fails and Narrant reports that failure without deleting the
  Snapshot record.
- Narrant may fetch objects required by the current Review Selection and may rely on
  normal remote-tracking refs, `FETCH_HEAD`, and linked-worktree metadata, but it does
  not create persistent refs solely to retain Review history.
- Deleting a Review does not require deleting Narrant-owned Git refs because none are
  created.

---
status: accepted
---

# Put source resolution outside local change content

Narrant keeps source-specific acquisition in Branch Selection and Pull Request Selection
resolvers. Local change content receives exact revisions and owns only immutable
comparison construction plus object reads pinned to one Snapshot.

Every public local Git I/O interface accepts `OperationContext`. Explicitly
non-cancellable work uses a non-cancellable context at the caller's policy seam rather
than a second convenience method.

## Consequences

- Review lifecycle resolves a Branch Selection through one call instead of coordinating
  fetch and comparison helpers.
- Pull Request special refs, remote fallback, and error classification live entirely in
  the Pull Request resolver.
- Branch ref discovery uses a dedicated concrete catalog with one `list` method; no trait
  is introduced for the sole Core Git implementation.
- Runtime readers are restored from immutable Snapshots and cannot resolve moving refs.
- Branch and Pull Request resolvers share one crate-private exact-comparison kernel.
- Removing local change content would redistribute diff parsing and object reading, not
  source-resolution protocols, across callers.

The governing requirements are in
[Narrow Local Change Content Interface](../specs/0011-narrow-change-comparison-content-interface.md).

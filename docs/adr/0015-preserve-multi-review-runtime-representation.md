---
status: accepted
---

# Preserve a multi-Review runtime representation

Keep the runtime representation capable of holding multiple open Reviews while the
application presents only one Review at a time. The Review lifecycle module retains
every Review opened during the current process so returning to one does not rebuild its
Review Workspace.

## Consequences

- Runtime Review state remains keyed by durable `reviewId`.
- Presentation remains single-Review, but navigation does not remove other Reviews from
  the runtime representation.
- Refactoring the Review lifecycle must not treat the multi-Review representation as
  accidental generality or collapse it to the Presented Review.
- Process exit releases every retained runtime entry and Review Workspace.

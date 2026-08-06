---
status: accepted
---

# Preserve a multi-Review runtime representation

Keep the runtime representation capable of holding multiple open Reviews even while
the first release permits only one active Review at a time. The Review lifecycle module
enforces the current product limit through its orchestration instead of reducing the
underlying representation to a single slot, because Narrant may later allow multiple
Reviews to remain open concurrently.

## Consequences

- Runtime Review state remains keyed by durable `reviewId`.
- Opening or switching Reviews continues to enforce the current single-active-Review
  behavior until the product model explicitly changes.
- Refactoring the Review lifecycle must not treat the multi-Review representation as
  accidental generality or collapse it solely because the first release opens one
  Review at a time.

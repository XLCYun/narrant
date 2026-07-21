---
status: accepted
---

# Encapsulate change content access

Expose file lists, individual file contents, and complete comparison materialization through one change-content module. Consumers such as Monaco, Tree-sitter, and AI analysis must not know whether content came from GitHub, local Git, a content-addressed disk cache, lazy loading, or background prefetch; this lets acquisition policy evolve without coupling it to presentation or analysis.

## Consequences

- The first release implements the abstraction with simple on-demand reads.
- Content-addressed caching, cancellable bounded background prefetch, byte budgets, and complete materialization remain optimizations behind the same interface.
- Keep source-specific GitHub and local-Git behavior behind adapters.
- Associate every read with an immutable comparison snapshot so a moving branch or updated pull request cannot mix revisions.

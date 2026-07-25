---
status: accepted
---

# Use current three-dot comparisons for every Review

Narrant calculates every Review Snapshot from the current commits resolved by its
Review Selection: Core Git finds their current merge base, and File Changes compare
that Comparison Base to the current head commit. Pull Request Selections use the same
rule as Branch Selections instead of importing GitHub's historical Pull Request diff
anchor; this keeps Pull Requests as collaboration-backed selectors rather than a second
code-comparison implementation, while accepting that Narrant may differ from GitHub's
Files changed page after the base branch evolves.

## Consequences

- Snapshot creation records the current base, comparison-base, and head object IDs.
- Pull Request metadata comes from GitHub, but File Changes and file content come from
  the local Core Git repository.
- Narrant describes its behavior as a current three-dot comparison and does not promise
  line-for-line equivalence with GitHub's Files changed page.

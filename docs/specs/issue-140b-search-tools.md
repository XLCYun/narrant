# Issue 140B: Deterministic Glob and Grep Tools

## Objective

Implement Glob and Grep behind the shared `babysit::repository_tools::RepositoryTools` public
registration/execution seam established by Issue 140A.

## Required behavior

- Glob traversal always starts at the canonical Review Workspace root; patterns are never used
  as host filesystem roots.
- Every Glob match independently passes the same canonical containment proof and is returned as
  a workspace-relative path.
- Glob ordering is deterministic and bounded to 500 paths with explicit truncation.
- Grep canonicalizes the requested file or directory target, searches only text files, emits
  workspace-relative path plus one-based line number and matching line text, and skips binary
  content safely.
- Grep ordering is deterministic and bounded to 500 matching lines with explicit truncation.
- Both operations observe `CancellationSignal` before and throughout traversal/search.
- Errors use stable categories/messages and never reveal the absolute workspace path.

## TDD acceptance seam

Tests live under `src-tauri/src/babysit/repository_tools/tests/` and invoke only the public
registration/execution seam against real temporary directory trees. Cover files/directories,
inside/outside symlinks, absolute and parent-component targets for Grep, deterministic ordering,
binary handling, both 500-result budgets, cancellation, and redacted errors. Run only focused
tests via `scripts/cargo-agent.sh`.


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
- Grep canonicalizes the requested file or directory target. Narrant proves every followed
  file target is contained by the canonical workspace before exposing a result; output keeps
  the workspace-relative logical traversal path, including a contained symlink's logical path.
- Grep uses ripgrep's `ignore`, `grep-regex`, and `grep-searcher` crates. `ignore::WalkBuilder`
  honors only `.gitignore` and `.ignore` rules found inside the Review Workspace. Ignore files
  above the workspace, global Git excludes, and `.git/info/exclude` are disabled. Hidden files
  and directories remain searchable unless a workspace-local rule ignores them.
- Grep streams each complete file with no per-file byte limit. Discovery of a NUL byte or
  invalid UTF-8 discards that file and all matches provisionally buffered from it.
- Grep emits workspace-relative logical path, one-based line number, and matching line text in
  deterministic logical-path then line order. A response stops at 500 matches or 64 KiB of
  final serialized response data, whichever comes first, and sets `truncated`. An over-budget
  final line may be shortened only at a valid UTF-8 boundary; there is no separate per-line
  limit.
- Grep accepts regex patterns up to 16 KiB and applies fixed compilation, DFA, and nesting
  resource limits. Compilation failure or any exceeded regex budget returns the stable
  invalid-pattern error.
- Both operations observe `CancellationSignal` before and throughout traversal/search. Grep
  checks between directory entries, on streamed reads, and in match callbacks, but does not
  guarantee interruption inside one regex buffer match.
- Errors use stable categories/messages and never reveal the absolute workspace path.

## TDD acceptance seam

Tests live under `src-tauri/src/babysit/repository_tools/tests/` and invoke only the public
registration/execution seam against real temporary directory trees. Cover files/directories,
inside/outside symlinks, absolute and parent-component targets for Grep, deterministic ordering,
workspace-local ignore rules, hidden paths, binary and invalid-UTF-8 handling, the 500-result and
64-KiB serialized-response budgets, oversized patterns, UTF-8-safe final-line truncation,
cancellation, and redacted errors. Run only focused tests via `scripts/cargo-agent.sh`.

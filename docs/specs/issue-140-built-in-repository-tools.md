# Issue 140: Built-in Repository Exploration Tools

## Problem Statement

A Built-in Agent Provider needs repository context, but giving a model generic
filesystem or shell access would violate the Review Workspace boundary and make tool
output unbounded.

## Solution

Provide reusable Narrant-owned Read, Glob, and Grep Rig tools rooted in one canonical
Review Workspace. Resolve every real target through the filesystem boundary, return
deterministically bounded results, expose continuation and truncation, and honor
Babysit cancellation.

## User Stories

1. As a reviewer, I want the agent to discover relevant files, so that explanations can
   use repository context.
2. As a security-conscious reviewer, I want every result confined to the Review
   Workspace, so that model input cannot escape the trusted checkout.
3. As a reviewer, I want explicit truncation, so that the model knows when a result is
   incomplete.
4. As a maintainer, I want deterministic bounded tools, so that provider contract tests
   do not depend on host ordering or repository size.
5. As a reviewer, I want cancellation to stop tool work, so that a cancelled analysis
   cannot continue reading repository content.

## Implementation Decisions

- The reusable tool set contains only Read, Glob, and Grep; it grants no write, edit,
  shell, Web, MCP, or arbitrary filesystem operation.
- Canonicalize the workspace root. Read and Grep canonicalize resolved targets and use
  component-aware `strip_prefix` to prove containment. Absolute paths and parent
  components are allowed only when their final canonical targets remain contained.
- Glob begins at the workspace root because patterns cannot be canonicalized, and each
  match must independently pass the canonical containment check.
- Grep uses the ripgrep `ignore`, `grep-regex`, and `grep-searcher` crates for file
  discovery, regular-expression matching, and streaming search. Narrant remains
  responsible for canonical containment, result ordering, and response budgets.
- Grep honors `.gitignore` and `.ignore` files contained in the Review Workspace. It
  does not use ignore files above the workspace root, the user's global Git excludes,
  or `.git/info/exclude`, and it does not exclude hidden paths merely because they are
  hidden.
- Grep streams complete files instead of imposing a per-file read budget. A file is
  discarded in full if the search observes a NUL byte or content that cannot be
  returned as valid UTF-8; matches buffered before that observation are not exposed.
- Read returns at most 64 KiB and accepts offset/limit continuation. Glob returns at
  most 500 paths. A Grep response returns at most 500 matches and 64 KiB of final
  serialized response data, whichever limit is reached first. If the final matching
  line does not fit, its text is cut only at a valid UTF-8 boundary. Reaching either
  limit sets `truncated`.
- Results use workspace-relative logical paths and deterministic path/line ordering,
  explicit `truncated`, and redacted stable errors without revealing the absolute
  workspace path. Following a symlink does not replace its logical result path; its
  canonical target must still remain inside the workspace.
- Grep checks the existing Babysit cancellation signal between directory entries, on
  streamed reads, and in match callbacks. It does not promise to interrupt one
  already-running regex buffer match.
- Grep limits the input regex pattern to 16 KiB and applies fixed regex compilation,
  DFA, and nesting resource limits. Invalid or over-budget patterns use the stable
  invalid-pattern error category.

## Testing Decisions

- Test the tool set at its public registration/execution seam with real temporary
  directories rather than private helper tests in production files.
- Cover normal relative paths, absolute and parent-component inputs resolving inside or
  outside, missing targets, files and directories, symlinks inside and outside,
  deterministic logical-path ordering, workspace-local ignore rules, hidden paths,
  binary/non-UTF-8 handling, offset continuation, result-count and serialized-byte
  budgets, oversized patterns, cancellation, and redacted errors.
- Rust tests follow the module `tests/` layout and run through the repository Cargo lock
  script.

## Out of Scope

- Provider clients, final Line Explanation output, shell-like glob expansion outside
  the workspace, writes, edits, model-hosted tools, or UI.

## Further Notes

- Governed by `docs/specs/0015-custom-babysit-provider.md` and ADR-0026.

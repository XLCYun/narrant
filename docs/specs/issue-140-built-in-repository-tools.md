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
- Read returns at most 64 KiB and accepts offset/limit continuation. Glob returns at
  most 500 paths; Grep returns at most 500 matching lines.
- Results use workspace-relative paths, deterministic ordering, explicit `truncated`,
  and redacted stable errors without revealing the absolute workspace path.
- Every potentially long operation observes the existing Babysit cancellation signal.

## Testing Decisions

- Test the tool set at its public registration/execution seam with real temporary
  directories rather than private helper tests in production files.
- Cover normal relative paths, absolute and parent-component inputs resolving inside or
  outside, missing targets, files and directories, symlinks inside and outside,
  deterministic ordering, binary/non-text handling, offset continuation, all three
  budgets, cancellation, and redacted errors.
- Rust tests follow the module `tests/` layout and run through the repository Cargo lock
  script.

## Out of Scope

- Provider clients, final Line Explanation output, shell-like glob expansion outside
  the workspace, ignored-file policy, writes, edits, model-hosted tools, or UI.

## Further Notes

- Governed by `docs/specs/0015-custom-babysit-provider.md` and ADR-0026.

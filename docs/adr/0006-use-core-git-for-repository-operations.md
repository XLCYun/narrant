---
status: accepted
---

# Use Core Git for repository operations

Narrant uses a Core Git executable as the single production implementation for
repository operations, including ref resolution, fetch, merge-base calculation,
diffing, and linked-worktree management. The first release uses the user's installed
Git and may later ship a controlled portable Git without changing the module interface;
this supersedes ADR-0003 because the product now requires an existing local clone and
must materialize the same checkout that the user's Git environment, credentials,
attributes, filters, and repository extensions produce.

## Consequences

- Read operations use Git's machine-oriented formats such as NUL-delimited output,
  explicit formats, porcelain output, and object IDs; human-readable stderr is
  diagnostic only and never drives business behavior.
- Repository operations execute Git directly with argument arrays rather than through
  a shell, and enforce cancellation, output limits, and postcondition checks.
- Narrant must define and probe a minimum supported Core Git capability set when a
  **Repository Entry** is added.
- Fetch uses the user's configured Core Git transport authentication. Narrant's GitHub
  App token remains confined to GitHub API operations and is not placed in command
  arguments, remote URLs, or Git configuration.
- After the user grants Workspace Trust as defined by ADR-0007, worktree creation uses
  the user's normal Git checkout behavior, including configured hooks, filters, and LFS
  integration.
- `git2-rs` is not retained as a second production source of Git semantics.

The supporting primary-source survey is recorded in
[system-git-vs-libgit2](../research/system-git-vs-libgit2.md).

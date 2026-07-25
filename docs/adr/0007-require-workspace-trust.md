---
status: accepted
---

# Require Workspace Trust before checkout

Narrant requires the user to grant durable Workspace Trust to a Repository Entry before
creating linked worktrees or starting review tooling. Normal Git checkout may execute
configured hooks and filters, access Git LFS, or perform other external actions, and
later review tools such as language servers may also act on repository-controlled
configuration; preserving the user's real development environment is more valuable
than silently producing a restricted checkout, so the UI explains these risks and
requires explicit consent before the first operation.

## Consequences

- Trust belongs to the Repository Entry rather than an individual Review and applies to
  future branch and Pull Request selections opened from that repository.
- Trust is stored in the application database. The first release does not provide a
  separate revoke action; deleting the Repository Entry removes its trust record.
- Before trust is granted, Narrant does not fetch, create a worktree, or start LSP or AI
  tooling for that Repository Entry.
- Worktree checkout follows the user's normal Core Git configuration rather than
  disabling hooks, filters, or LFS behavior.

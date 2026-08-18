---
status: accepted
---

# Resolve Pull Request Selections behind one seam

Narrant resolves a durable Pull Request Selection through one backend module before
Review lifecycle persists or presents a Snapshot. The resolver owns current GitHub
observation, matching base-repository remotes, fetching the base branch and GitHub's
read-only `refs/pull/<number>/head`, validating exact revisions, calculating the merge
base, and constructing the local Change Comparison.

The resolver uses the Pull Request head ref rather than GitHub's synthetic merge ref.
Narrant therefore preserves its source-independent three-dot comparison semantics and
presents a Review Workspace at the actual Pull Request head revision.

## Consequences

- Callers provide only GitHub repository identity and pull-request number; moving refs,
  commit SHAs, metadata, remote names, and special refs do not cross the resolver seam.
- Pull Request discovery remains a UI listing concern and is not reused as resolution.
- Review lifecycle calls the resolver but continues to own durable Reviews, candidate
  staging, Workspace convergence, CAS activation, rollback, and runtime readers.
- Core Git fetches objects without persisting `refs/pull/*`, changing configured fetch
  refspecs, creating local branches, or writing `FETCH_HEAD`.
- GitHub API access remains asynchronous, so Review lifecycle operations that may resolve
  a Pull Request are asynchronous even when a Branch Selection uses only local processes.
- Resolver tests use a local HTTP server and real Git repositories rather than exposing
  GitHub wire or Core Git implementation seams through the public interface.

The governing requirements are in
[Pull Request Selection Resolution](../specs/0009-pull-request-selection-resolution.md).

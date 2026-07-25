# Stateful Review Sessions

> **Superseded:** The runtime Review Session model and `sessionId` contract are replaced
> by the durable Review and temporary Review Workspace model in
> [0003 Local Repository Reviews](0003-local-repository-reviews.md). This document is
> retained as implementation history.

## Problem Statement

Opening a Review Snapshot currently stores only its serialized description and enough
source information to reconstruct access later. Reading a Local Git File Change reopens
the repository and resolves commits and trees again; reading a GitHub File Change
recreates its adapter and HTTP client. The Review Session therefore does not own the
acquisition state created when its immutable Review Snapshot is opened, and there is no
frontend operation for releasing that state explicitly.

## Solution

Opening a Local Branch Comparison or Pull Request creates a stateful backend Review
Session. The session owns its source adapter, immutable Review Snapshot, and any
source-specific acquisition metadata needed for on-demand File Change reads. The
frontend receives tagged command-response models mapped from source-specific internal
snapshots and uses their session ID for subsequent reads.

For Local Git, comparison traversal records the before and after Git object IDs for each
File Change without retaining file text. Reading a File Change finds it by its public
`path` and materializes text directly from those object IDs. For GitHub, the opened
session retains the adapter and HTTP client used to open the Pull Request.

The frontend can explicitly close a Review Session. Closing is source-independent,
idempotent, prevents new reads, and releases acquisition state after any read already in
progress completes.

## User Stories

1. As a reviewer, I want opening a Local Branch Comparison to retain its opened repository state, so that selecting files does not repeatedly reopen the repository.
2. As a reviewer, I want File Changes to remain pinned to the immutable Review Snapshot, so that moving branches cannot mix revisions during review.
3. As a reviewer, I want file text loaded only when I select a File Change, so that opening a large comparison does not retain every file body in memory.
4. As a reviewer, I want repeated reads to resolve through the opened Review Session, so that acquisition details remain behind the change-content seam.
5. As a reviewer, I want renamed files addressed by their new public path, so that every File Change has one stable lookup identity.
6. As a reviewer, I want deleted files addressed by their deleted path, so that I can still select and inspect their before content.
7. As a reviewer, I want binary, symlink, submodule, and text metadata preserved, so that on-demand loading does not change presentation behavior.
8. As a reviewer, I want Pull Request reads to reuse the adapter opened for that Review Session, so that HTTP acquisition state follows the session lifecycle.
9. As a reviewer, I want replacing an open comparison to close the previous Review Session, so that abandoned acquisition state does not accumulate.
10. As a reviewer, I want leaving the application view to close its active Review Session, so that resources are released when the review is no longer visible.
11. As a reviewer, I want closing a Review Session more than once to succeed, so that UI cleanup can be race-safe and simple.
12. As a reviewer, I want reads after a session is closed to fail with a Review Session recovery message, so that I know to reopen the comparison.
13. As a reviewer, I want a close request not to interrupt a File Change read already in progress, so that closing does not create partial results.
14. As a maintainer, I want Local Git delta interpretation implemented once, so that path, rename, status, kind, and size rules cannot drift between listing and reading.
15. As a maintainer, I want frontend interaction models separated from Local Git acquisition models, so that Git object IDs do not leak across the source-independent seam.
16. As a maintainer, I want tests to exercise the same Review Session interfaces used by callers, so that implementation-only seams do not become a second behavior contract.

## Implementation Decisions

- Rename the application state module from change-content state to `ReviewSessionStore`
  and store opened Review Sessions by session ID.
- Store each opened review behind a reference-counted session lock. The global store
  lock is held only while inserting, looking up, or removing a session.
- Closing removes the session from the store immediately. A read that already acquired
  a reference may finish; new reads fail.
- Make closing idempotent. Missing IDs are successful close operations.
- Represent opened sessions as source-specific variants for Local Git and GitHub.
- A Local Opened Review directly owns its repository and Local Snapshot; the former
  Local Git change-content holder is removed.
- A Local Snapshot has no session or snapshot ID. It stores typed Git object IDs for
  revisions and an ordered collection of Local File Changes.
- A Local File Change stores path, old path, status, and optional before and after Local
  Entries. It does not store file text, content kind, or sizes.
- A Local Entry stores only its Git object ID and tree-known entry kind. Binary/text
  detection, size calculation, and text decoding happen only when the File Change is
  read.
- Do not maintain a separate path index. File Change reads linearly search the ordered
  collection by metadata path; an index may be added only after profiling demonstrates
  a need.
- The public File Change path is the only lookup key. A renamed file uses its new path;
  its old path remains metadata. A deleted file uses its deleted path.
- Reading Local Git text resolves blob content directly by object ID. Submodule content
  is the stored commit object ID. Binary content remains absent.
- Local Git and GitHub use separate source-specific internal snapshot and File Change
  models. These models do not derive serialization and do not import command-response
  types.
- Reading produces a short-lived loaded File Change. File text is not written back into
  the stored snapshot and no content cache is introduced.
- The command/presentation seam owns all serialized response types and mappers.
- Snapshot responses form a tagged Local/GitHub union and carry `sessionId`; internal
  snapshots carry no runtime identity.
- Snapshot File Change summaries contain only path, old path, and status. Loaded File
  Change responses additionally contain content kind, sizes, and before/after text.
- Remove `snapshotId` from the frontend and backend contract without a compatibility
  alias. Read and close commands accept `sessionId`.
- Base Revision is the commit resolved from Base Ref, Head Revision is the commit
  resolved from Head Ref, and Comparison Base is their merge-base commit. File Changes
  compare Comparison Base to Head Revision.
- A GitHub Review retains the GitHub adapter and HTTP client created when the Pull
  Request is opened. Its existing network and on-demand loading behavior remains
  unchanged.
- Expose a source-independent `close_review_session(sessionId)` frontend command.
- When the frontend replaces or clears an active Review Snapshot, it closes the prior
  Review Session. Component cleanup also closes the active session.
- Existing ADR requirements remain in force: source behavior stays behind adapters,
  snapshots remain immutable, and Local Git access continues through embedded
  `git2-rs`.

## Testing Decisions

- Test observable behavior through the Review Session commands and opened-review
  interfaces rather than private Git traversal helpers.
- Preserve existing Local Branch Comparison coverage for merge-base semantics,
  added/modified/deleted/renamed changes, binary files, LFS pointers, symlinks,
  submodules, and exclusion of staged, unstaged, and untracked worktree content.
- Verify a renamed File Change can be read by its new path and cannot be read by its old
  path.
- Verify Local Git content is still read from pinned object IDs after refs move.
- Verify closing a Review Session makes later reads return the expired-snapshot error.
- Verify closing a missing or already-closed Review Session succeeds.
- Verify a close racing with an already-started read does not invalidate that read.
- Verify opening a replacement Review Snapshot closes the previous session from the
  frontend.
- Verify frontend cleanup closes the active session.
- Preserve the existing GitHub Pull Request tests as prior art for source-independent,
  on-demand File Change reads.
- Produce Rust and frontend coverage reports, inspect uncovered lines and branches, and
  add valuable tests across statuses, entry kinds, mapper variants, lifecycle errors,
  serialization, and frontend cleanup. Do not add production interfaces solely for
  testing and do not impose an arbitrary percentage threshold.
- Run the complete Rust and frontend test suites.

## Out of Scope

- Prefetching or complete File Change materialization.
- Retaining hydrated file text in Review Session memory.
- A content-addressed disk cache or in-memory LRU.
- A separate path-to-index map without profiling evidence.
- Concurrent reads within one Review Session beyond the session-level lock.
- Changing GitHub authentication or request behavior.
- Introducing a system Git CLI.
- Automatically refreshing a Review Snapshot when refs or a Pull Request move.

## Further Notes

Each source-specific internal Review Snapshot describes an immutable Change Comparison
without owning transport identity. The backend Review Session owns source handles, and
the command/presentation seam maps the internal snapshot into a tagged serialized
response carrying the session ID.

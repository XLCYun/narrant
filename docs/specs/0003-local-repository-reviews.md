---
status: accepted
---

# Local Repository Reviews

## Problem Statement

Narrant currently treats a local branch comparison and a GitHub Pull Request as two
code sources. GitHub-backed Reviews can display remote File Changes without a complete
local repository, while future language-server and AI analysis require a coherent
project checkout. The current runtime Review Session disappears at application exit,
has no durable repository or Review history, and makes source acquisition determine
the frontend model.

Narrant will require an existing local clone. GitHub remains the collaboration platform
for Pull Request discovery and metadata, while Core Git becomes the sole source of
repository history, File Changes, file content, and Review Workspaces.

## Product Model

The application has a repository-first information architecture:

```text
Repository
├── Workspace Trust
├── available Branch Selections
├── GitHub remotes and their Pull Requests
└── Reviews
    ├── Review Selection
    ├── Review Snapshots
    └── temporary Review Workspace while open
```

The home view lists registered Repositories and a cross-repository recent-Review
projection. Opening a Repository shows its saved Reviews and the two ways to
create or reopen one:

1. Select a local or remote-tracking base/head branch pair.
2. Select a Pull Request grouped under one of the repository's GitHub remotes.

Both selections resolve current base and head commits and then enter one local Review
pipeline.

## Domain Rules

- A Repository identifies one concrete local clone by its saved canonical path.
  Another clone is a different entry even when both have identical remotes.
- A Review belongs to exactly one Repository and owns one immutable Review
  Selection identity.
- A Branch Selection is identified by canonical `refs/heads/*` or
  `refs/remotes/*` base and head refs. Arbitrary revspecs, tags, and directly entered
  object IDs are not accepted in the first release.
- A Pull Request Selection is identified by the base GitHub repository identity and
  Pull Request number.
- The same Selection within one Repository reopens the existing Review rather than
  creating a duplicate.
- Changing a Branch Selection creates another Review. A Pull Request changing its base
  branch remains the same Review because its Pull Request identity did not change.
- A Review contains successive immutable Review Snapshots. Each Snapshot records exact
  base, comparison-base, and head object IDs plus stable File Change metadata.
- File Changes compare the current merge base of base/head to the current head commit.
  The head commit determines the complete Review Workspace checkout.
- Narrant does not reproduce GitHub's historical Pull Request comparison anchor and
  does not promise line-for-line equivalence with GitHub's Files changed page.
- Only one Review is open at a time in the first release.

The canonical vocabulary and relationships remain defined in the repository root
`GLOSSARY.md`.

## Repository Persistence

Narrant stores one application-level SQLite database in the operating system's
application-data directory. It persists:

- Repositories, canonical paths, creation time, and last-opened time;
- Workspace Trust;
- Reviews and their last-opened ordering;
- tagged Branch or Pull Request Selections;
- Review Snapshots and the current Snapshot pointer;
- base, comparison-base, and head object IDs;
- stable File Change metadata such as path, old path, status, modes, and object IDs
  needed for on-demand reads;
- cached Pull Request identity and basic display metadata: repository, number, title,
  description, author, state, draft state, URL, and last synchronization time.

Secrets remain in the operating-system credential store. Worktrees and future large
artifacts remain ordinary files outside SQLite.

No persisted Review data exists in previous releases, so the first schema does not
import legacy runtime Review Sessions. Existing GitHub credentials remain in their
current system credential slot.

## Repository Lifecycle

Adding a Repository:

1. Canonicalizes and validates the selected path as a working-tree Git repository.
2. Locates Core Git, probes the capabilities required by this specification, and
   reports an actionable incompatibility when they are unavailable.
3. Reads local branches, remote-tracking branches, and configured remotes without
   changing repository state.
4. Stores the entry in SQLite.
5. Presents Workspace Trust before the first fetch, worktree checkout, or review-tool
   startup.

Workspace Trust applies to the whole Repository and future Reviews created from
it. The warning explains that normal checkout may execute Git hooks and filters, access
Git LFS, and later allow language servers or other review tools to act on
repository-controlled configuration. The first release has no separate revoke action;
future deletion of the Repository removes its trust record.

If the saved local path no longer exists, the Repository and Review history remain
visible but unavailable. A future relocate action may update the path after validating
another local repository. Matching remotes do not automatically merge separate clones.

## GitHub Pull Request Discovery

Narrant normalizes every GitHub HTTPS or SSH remote in a Repository, de-duplicates
the represented GitHub repositories, and groups their accessible Pull Requests by
repository. A Repository does not have one exclusive GitHub association.

The GitHub App uses only:

- Metadata: read;
- Pull requests: read.

Anonymous discovery remains available for public repositories. Device Flow and stored
GitHub credentials enable private Pull Request discovery. GitHub credentials are used
only for GitHub API operations and are never passed to Core Git.

Selecting a Pull Request persists its base repository identity and Pull Request number.
Checking that Review for updates asks GitHub for its current base/head refs and object
IDs. Core Git fetch uses the user's existing remote transport, SSH configuration,
credential helpers, proxy, and certificate environment.

Pull Request metadata failures and Git transport failures are distinct. A successful
GitHub API request does not imply that Core Git can fetch the corresponding objects.

GitHub sign-out removes access and refresh credentials plus pending authorization state.
It does not delete Repositories, Reviews, Snapshots, worktrees, cached Pull
Request metadata, or recent-Review history.

## Core Git Repository Operations

A deep Repository Operations module is the sole production source of Git semantics. Its
Core Git process adapter covers:

- repository and common-directory discovery;
- canonical local and remote-tracking branch enumeration;
- remote enumeration and URL normalization inputs;
- ref-to-commit resolution;
- fetch;
- merge-base calculation;
- File Change calculation and rename detection;
- object and file-content reads;
- linked-worktree creation, update, inspection, and removal.

The adapter executes Core Git directly with argument arrays, never through a shell.
Normal data comes from object IDs, explicit formats, NUL-delimited records, raw diff
formats, and porcelain formats. Localized stderr and progress output are diagnostic and
never drive business logic.

Commands have cancellation, timeout, and output limits. Every destructive operation
validates the exact Repository, Git common directory, registered worktree, Review
identity, and Narrant-managed absolute path before execution.

`git2-rs` is removed as a production source of Git semantics rather than retained as a
second implementation.

## Snapshot Creation and Refresh

Creating the first Snapshot:

1. Resolve or fetch the Selection's current base and head commits.
2. Calculate their current merge base.
3. Calculate File Changes from comparison base to head.
4. Persist the immutable Snapshot and File Change metadata.
5. Create the Review Workspace at the exact head commit.
6. Make the Snapshot current only after persisted state and workspace agree.

Opening an existing Review uses its current Snapshot and lazily creates the Review
Workspace. If required Git objects have disappeared, the Core Git materialization
operation fails naturally and the saved Review history remains.

Checking for updates does not change the current Snapshot:

- local branches are re-resolved from local refs;
- remote-tracking branches fetch their selected remotes before re-resolution;
- Pull Requests query GitHub for current identities and fetch required objects through
  Core Git.

When resolved commits differ, the UI offers an explicit refresh. Refresh never changes
the Review silently. A successful refresh:

1. creates a new immutable Snapshot;
2. forcibly restores the disposable Review Workspace and checks out the new head;
3. atomically advances the current Snapshot from the user's perspective;
4. presents the new File Changes.

The Review Reader keeps one user-visible update action available for every presented
Branch or Pull Request Review. Opening, switching, or restoring a Review does not check
for updates, and a failed switch does not restart a cancelled check. Clicking the action
explicitly checks the presented Review and refreshes it if an update is available.
Checking and refreshing disable that action for the operation's duration, and duplicate
requests do not replace an operation already in flight. A successful refresh restores
the manual check action without immediately checking again.

A no-update result remains visible until the next check or Snapshot transition. Check
and refresh failures use non-blocking notifications while the action retains the
appropriate retry state. If the confirmed upstream revisions move before refresh, the
confirmation is discarded and the action requires another check; other refresh failures
retain the confirmation for retry. Switching Review Views preserves update state, while
leaving the presented Review cancels an update check. Narrant performs no background
polling.

A failure before the core transition completes leaves the old Snapshot current.
Language-server or AI-tool startup occurs afterward and may fail without rolling the
Review back.

Historical Snapshot metadata remains in SQLite. Narrant creates no persistent
`refs/narrant/*` namespace to retain its Git objects. Branch deletion, force-push,
pruning, or garbage collection may therefore make historical content impossible to
materialize; later failure does not delete or mark the saved Snapshot proactively.

## Review Workspace Lifecycle

An open Review owns:

```text
<application-data>/Narrant/reviews/<review-id>/
└── repo/   # detached linked worktree at the current head commit
```

Future durable artifacts and reusable tool state may use sibling directories, but
`repo/` contains no user-owned state. Narrant may use forced checkout, hard reset, and
clean operations to discard tracked, untracked, and ignored changes when refreshing or
rebuilding it.

The Workspace is created lazily and removed when:

- the application exits normally.

Every Review opened during the current application process retains its Workspace.
Switching presentation or navigating away does not remove it, and there is no automatic
count, disk, or idle-time eviction policy. See
[Spec 0012](0012-process-lifetime-open-reviews.md).

On startup, Narrant removes worktrees left by an interrupted prior run only when SQLite,
the Git common directory, the registered linked-worktree path, and the expected
Narrant-managed absolute path all identify the same workspace. It does not run a broad
`git worktree prune` over unrelated worktrees.

## Presentation and Runtime Interface

The frontend consumes one source-independent Review model. Pull Request metadata is an
optional collaboration projection attached to a Pull Request Selection; Snapshot and
File Change models are not tagged as Local/GitHub code sources.

Runtime commands use durable `reviewId` and immutable Snapshot identity. They do not
expose a separate `sessionId`. Runtime entries remain open until process shutdown;
closing remains an internal idempotent primitive for future explicit close or Repository
deletion without deleting SQLite history.

File Change summaries are available from persisted Snapshot metadata. Before content is
read from the comparison-base object and after content from the head object or current
Review Workspace. Binary, symlink, executable-mode, rename, deletion, and submodule
metadata remain distinguishable.

## User Stories

1. As a reviewer, I want to register a local clone and see all saved Reviews associated
   with it.
2. As a reviewer, I want a recent-Review list across repositories for quick reopening.
3. As a reviewer, I want to grant Workspace Trust before Narrant performs normal
   checkout behavior.
4. As a reviewer, I want to create a Review from two canonical branches.
5. As a reviewer, I want Pull Requests grouped under every GitHub remote in my local
   repository.
6. As a reviewer, I want choosing a Pull Request to use its current base/head commits
   while preserving its title and other collaboration metadata.
7. As a reviewer, I want Branch and Pull Request Reviews to produce the same local
   three-dot File Change semantics.
8. As a reviewer, I want reopening the same Selection to resume one durable Review
   rather than create duplicates.
9. As a reviewer, I want ref movement detected without changing the code I am currently
   reviewing.
10. As a reviewer, I want explicit refresh to preserve the previous Snapshot and advance
    the Review to the new code version.
11. As a reviewer, I want the displayed after-code, Review Workspace, and future
    language-server view to refer to the same head commit.
12. As a reviewer, I want GitHub sign-out to preserve my local Review history.
13. As a security-conscious reviewer, I want Narrant's destructive cleanup limited to
    worktrees it can prove it owns.
14. As a maintainer, I want Core Git machine formats and postconditions instead of
    localized human-output parsing.
15. As a maintainer, I want optional analysis tools to degrade independently from the
    core Review state.

## Cross-ticket Acceptance Criteria

- Repositories and Reviews survive a complete application restart.
- Two clones with the same remote create separate Repositories.
- Duplicate Branch or Pull Request Selections reopen the existing Review.
- Branch selectors expose only canonical local and remote-tracking branches and exclude
  Narrant-internal refs.
- Multiple GitHub remotes produce de-duplicated, repository-grouped Pull Request lists.
- GitHub API tests demonstrate that code-content endpoints are never called.
- GitHub sign-out deletes credentials but preserves persisted Reviews.
- Core Git tests cover spaces, newlines, Unicode paths, rename, deletion, binary files,
  executable-mode changes, symlinks, submodules, shallow clones, and diverged branches
  without parsing localized human output.
- File Changes equal `merge-base(current-base, current-head) → current-head`.
- An update check never changes the current Snapshot or Review Workspace.
- A failed refresh leaves the previous Snapshot current.
- A successful refresh makes SQLite, File Changes, and detached head worktree agree on
  the same head object ID.
- Optional analysis-tool failure does not roll back a successful refresh.
- Switching Reviews retains every open linked worktree; normal application exit removes
  the complete Open Review Set.
- Startup cleanup leaves unrelated user and third-party worktrees untouched.
- Workspace rebuild may discard dirty and ignored contents only after exact ownership
  validation.
- No `refs/narrant/*` entries are created.
- GitHub App registration and tests require Metadata and Pull requests read permissions,
  not Contents permission.
- Rust and frontend tests, Clippy, ESLint, TypeScript build, and required coverage review
  complete successfully.

## Out of Scope

- GitHub conversation comments, review comments, review submission, or mutation.
- Language-server installation, process management, indexing, or navigation.
- AI code reading, AI Findings, or model integration.
- Dependency installation, builds, tests, code generation, or automatic submodule
  initialization.
- Repository, Review, or Snapshot deletion UI.
- Workspace Trust revocation UI.
- Historical Snapshot browsing UI.
- Arbitrary revision expressions, tags, or direct object-ID selections.
- Multiple simultaneously presented Review Readers or Review tabs.
- Exact reproduction of GitHub's historical Pull Request Files changed anchor.
- A Narrant backend, client secret, private key, or Git transport credential injection.

## Superseded Design

This specification replaces the source-specific GitHub Change Content and runtime
Review Session architecture in Spec 0001, ADR-0002, and ADR-0005. Spec 0002 remains the
source of truth for Octocrab-backed Device Flow, refresh-token rotation, secret
handling, and system credential storage only; its GitHub Source, Review Session,
sign-out cleanup, and permission requirements no longer apply.

The implementation is intentionally allowed to replace existing frontend commands and
models without compatibility aliases because prior Review Sessions were transient and
no persisted Review persistence exists.

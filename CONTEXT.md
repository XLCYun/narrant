# Change Review

This context presents changes between a base revision and a head revision as a navigable set of file changes, independent of where those revisions originate.

## Language

**Pull Request**:
A GitHub collaboration record that selects the base and head revisions for a
repository-local **Change Comparison** and carries collaboration metadata.
_Avoid_: PR data, merge request

**Review Selection**:
The durable choice a **Review** uses to resolve its moving **Base Ref** and
**Head Ref**. It is either a **Pull Request Selection** or a **Branch Selection**.
_Avoid_: Review source, snapshot

**Pull Request Selection**:
A **Review Selection** identified by a GitHub repository and pull-request number.
It retains that identity for ref resolution and collaboration-metadata synchronization.
_Avoid_: GitHub comparison

**Branch Selection**:
A **Review Selection** identified by canonical local or remote-tracking base and
head branch refs in one **Repository**.
_Avoid_: Local PR

**Branch Selection Continuation**:
The application decision that matches a selected **Branch Selection** to its existing
**Review**, checks for a **Review Update**, and chooses whether to start, continue, or
explicitly refresh that Review.
_Avoid_: Branch Review session, comparison mode

**Local Branch Comparison**:
A **Change Comparison** whose base and head revisions come from a local Git repository.
_Avoid_: Local PR, offline PR

**Change Comparison**:
The ordered difference from a comparison base revision to a head revision.
_Avoid_: PR, diff

**Base Ref**:
A canonical local or remote-tracking branch ref into which the proposed changes
would be integrated.
_Avoid_: Target, old branch

**Head Ref**:
A canonical local or remote-tracking branch ref containing the proposed changes
under review.
_Avoid_: Source, new branch

**Comparison Base**:
The common ancestor of the **Base Ref** and **Head Ref** from which reviewed changes are calculated.
_Avoid_: Base Ref, base tip

**File Change**:
The before-and-after content and change metadata for one path in a **Change Comparison**.
_Avoid_: Diff file, patch item

**File Content Reference**:
An immutable reference from one side of a **File Change** to content pinned by a
**Change Comparison**'s exact revisions and retained if that comparison becomes a
**Review Snapshot**. A File Change may have a before reference, an after reference, or both.
_Avoid_: Entry, File Entry, File Version

**Review Reader**:
The application view that presents one open **Review Snapshot**, including its
collection of **File Changes** and the freely selected **File Change** content. It
does not assign reading progress or an approval state to File Changes.
_Avoid_: File Changes page, Diff page

**Review View**:
One peer presentation of the **Review Snapshot** currently open in the **Review Reader**.
The available views are Overview and File Changes; switching views does not create another Review.
_Avoid_: Review mode, sidebar panel

**Review Snapshot**:
An immutable **Change Comparison** that became current for a **Review**, pinned to exact
base, comparison-base, and head revisions. Its identity and history remain saved even
when its Git objects are no longer available in the user's repository.
_Avoid_: Latest PR, live diff

**Review Refresh Candidate**:
An immutable **Change Comparison** prepared by an explicit refresh but not yet made
current for its **Review**. It becomes a **Review Snapshot** only after the refresh
transition succeeds; a failed candidate is not part of Review history.
_Avoid_: Pending Snapshot, draft Snapshot

**Review Update**:
The condition in which the moving base or head ref of a **Review Selection** no
longer resolves to the revisions presented by the open **Review Snapshot**.
_Avoid_: Available version, new Snapshot

**Review**:
A durable code-review record owned by one **Repository**. It preserves review
history across application launches, owns one **Review Selection**, and may
contain successive **Review Snapshots** as those refs move.
Opening it activates the runtime resources needed to inspect its current snapshot.
The same **Review** cannot be opened more than once concurrently.
_Avoid_: Review Session, cached review

**Repository**:
A specific locally cloned Git repository registered with Narrant as the home of its
saved review history. Another clone remains a distinct Repository even when both
clones refer to the same remote repository.
_Avoid_: Project, remote repository

**Repository Application**:
The deep application module that projects Repositories, Workspace Trust, and
saved Review summaries into one enterable Repository Overview. It resolves availability
and Trust before exposing the overview and provides the canonical path needed by local
Git and Pull Request discovery operations.
_Avoid_: Catalog adapter, storage service

**Repository Overview**:
The application projection presented after one Repository passes availability and
Workspace Trust checks. It contains that Repository and its visible Reviews.
_Avoid_: Repository Context, Repository Session, Catalog row

**Workspace Trust**:
A reviewer's durable authorization for Narrant to perform normal Git checkout and
run review tooling for one **Repository**.
_Avoid_: Safe repository, Review permission

**GitHub Identity**:
The single GitHub user currently authorized to act through Narrant's GitHub App.
It is an external authorization identity, not a Narrant-owned user account.
_Avoid_: Narrant Account, User Profile

**Pull Request Discovery**:
The current observation of accessible **Pull Requests** grouped by the GitHub repositories
represented by one **Repository**'s remotes. It may be anonymous or use the current
**GitHub Identity**.
_Avoid_: PR list, Repository GitHub association

**UI Icon**:
A compact visual symbol that communicates an interface action, object, or status.
It excludes product and third-party logos, application icons, avatars, and
content-provided graphics.
_Avoid_: Logo, brand icon

**File Type Icon**:
A **UI Icon** that identifies a File Change's file type from its path. It is
separate from the icon that communicates the File Change status.
_Avoid_: File Change status icon

**Review Workspace**:
The Narrant-owned, disposable checkout of the head revision presented by one
open **Review**. It contains no user-owned state, may be forcibly rebuilt when the
review advances to another snapshot, and is removed when the review closes.
_Avoid_: Repository, user worktree

**Babysit Mode**:
An optional Review Reader presentation that helps a reviewer understand a Review
Snapshot by showing AI-generated Line Explanations. It explains code without judging
its quality, reporting findings, or proposing review decisions.
_Avoid_: AI Review, review agent, beginner mode

**Line Explanation**:
An AI-generated explanation that helps a reviewer understand one non-empty code line
on either side of a File Change in one Review Snapshot. Babysit Mode may filter which
Line Explanations are visible without changing their meaning or source identity.
It is derived review data and never modifies the File Change or Review Workspace source text.
_Avoid_: Source comment, Review Comment, AI Finding

**Babysit Regeneration**:
An explicit reviewer action that asks the Babysit Agent Provider for a new set of Line
Explanations for the same File Change identity and replaces the reusable derived result.
It is distinct from Retry, which recovers a failed analysis.
_Avoid_: Refresh, Retry, cache clear

**Babysit Agent Provider**:
A source of Line Explanations that fulfills Narrant's provider-independent analysis
contract. It is either an Installed Coding Agent Provider or a Built-in Agent Provider.
_Avoid_: Model, AI backend

**Installed Coding Agent Provider**:
A Babysit Agent Provider that delegates analysis to a supported coding-agent runtime
already installed and authenticated by the reviewer.
_Avoid_: Attached agent, external model

**Codex Environment Reuse**:
The Installed Coding Agent behavior in which Narrant asks the current executable
environment for Codex before falling back to a reviewer-selected executable, and lets
the resulting App Server use that reviewer-owned configuration and authentication state.
_Avoid_: Credential Bridging, Isolated Codex Home

**Built-in Agent Provider**:
A Babysit Agent Provider whose agent loop and read-only tools are owned by Narrant and
whose configured model service supplies inference.
_Avoid_: Installed agent, native model

## Relationships

- A **Pull Request** selects the base and head revisions for one repository-local
  **Change Comparison**
- A **Review Selection** resolves one moving **Base Ref** and **Head Ref** pair
- A **Pull Request Selection** retains its pull-request identity while resolving the
  current refs selected by that pull request
- A **Branch Selection** retains canonical local ref identities
- A **Branch Selection Continuation** never changes the selected refs; it decides how
  the resulting Branch Selection opens its unique Review
- A **Local Branch Comparison** identifies one repository-local **Change Comparison**
- A **Change Comparison** uses the merge base of its **Base Ref** and **Head Ref** as its **Comparison Base**
- Both **Branch Selections** and **Pull Request Selections** calculate their
  **Comparison Base** from the current base and head commits; a Pull Request does not
  import GitHub's historical Files changed anchor
- A **Change Comparison** contains zero or more **File Changes**
- A **Review Reader** presents one open **Review Snapshot** and one selected **File Change** at a time
- A **Review Reader** provides Overview and File Changes as peer **Review Views** over the same open **Review Snapshot**
- Selecting a **File Change** in the **Review Reader** is navigation only; it does not
  mark that change as viewed, reviewed, or approved
- A **Repository** owns zero or more **Reviews**
- A **Repository Application** owns the enterable **Repository Overview** projection
  for one or more **Repositories**
- A **Repository Overview** contains one collection of **Reviews**; a Review's Selection,
  current Snapshot revisions, and list metadata do not form separate Review collections
- A current **Review Snapshot** summary identifies its exact base, Comparison Base, and
  head revisions even when a particular overview uses only part of that identity
- A Review summary's last-opened time is list activity metadata; it does not belong to
  the Review Selection or the current Review Snapshot
- A **Repository Overview** and a cross-Repository recent-review aggregate expose only
  Reviews with an activated current **Review Snapshot**; an unactivated persistence
  record is not a visible Review
- A **Review** inside a **Repository Overview** inherits that overview's Repository
  identity; only cross-Repository aggregate views identify the owning Repository
- Narrant has at most one current **GitHub Identity**
- A **GitHub Identity** may be represented by cached public profile information while
  GitHub is temporarily unavailable, but signing out removes that representation
- A **GitHub Identity** change invalidates the current **Pull Request Discovery**, but
  navigating between **Repositories** does not refresh the **GitHub Identity**
- Moving a local clone does not create a new **Repository** when the user
  explicitly relocates its saved path
- A **Repository** may discover pull requests from multiple GitHub repositories
  represented by its Git remotes; it does not own one exclusive GitHub association
- A **Repository** must have **Workspace Trust** before Narrant checks out code or
  starts review tooling for it
- Deleting a **Repository** also deletes its **Workspace Trust**
- A **Review** contains one or more successive **Review Snapshots**
- A **Review Refresh Candidate** becomes a **Review Snapshot** only after its Review
  Workspace and current pointer consistently represent it
- A failed **Review Refresh Candidate** never appears in Review history
- Crash recovery restores the last successfully activated current **Review Snapshot**;
  it never activates an unconfirmed **Review Refresh Candidate**
- Historical **Review Snapshots** do not keep their Git objects alive and may become
  unavailable after the user's repository discards those objects
- A **Review** owns exactly one **Review Selection**, while each **Review Snapshot**
  pins the exact commits resolved from that selection at one moment
- A **Review Selection** is represented explicitly as either a **Branch Selection** or
  a **Pull Request Selection**; missing Pull Request metadata does not identify its kind
- A **Pull Request Selection** may carry its persisted Pull Request collaboration metadata
  as one value; separating that cache from Selection identity is outside the current model
- A **Review Selection** identity never changes after its **Review** is created; choosing
  another branch pair creates another **Review**
- Within one **Repository**, the same **Review Selection** reopens its existing
  **Review** rather than creating a duplicate
- An open **Review** owns one disposable **Review Workspace** whose repository checkout
  presents the head revision of its current **Review Snapshot**
- **Babysit Mode** presents **Line Explanations** derived from one exact Review Snapshot
- A **Line Explanation** does not modify the source text presented by its File Change
- Every **Babysit Agent Provider** produces the same provider-independent Line Explanation contract
- An **Installed Coding Agent Provider** reuses a reviewer-installed agent runtime and its authentication
- A **Built-in Agent Provider** never grants its model service direct access to the Review Workspace
- An open **Review** presents exactly one **Review Snapshot** at a time
- Ref movement makes a newer snapshot available but never changes the snapshot being
  reviewed silently
- Checking a **Review** for updates may fetch its selected remote refs or query its
  **Pull Request**, but does not change the presented **Review Snapshot**
- Refreshing a **Review** adds and presents a newer **Review Snapshot** only after
  explicit user action
- A refresh becomes current only after its **Review Snapshot**, **File Changes**, and
  **Review Workspace** consistently represent the new head revision
- Failure of optional analysis tooling does not roll back a current
  **Review Snapshot**
- Closing a **Review** releases runtime resources without deleting its saved history
- A **Review** remains closed when cleanup of its **Review Workspace** fails; the
  leftover workspace is retained only for a later safe recovery attempt
- A recent-review list is an aggregate view of **Reviews** across **Repositories**;
  each entry identifies both the Review and its owning Repository

## Example dialogue

> **Dev:** “If new commits are pushed to a **Pull Request** while its **Review** is open, should its **File Changes** update automatically?”
> **Domain expert:** “Not silently—the review presents a specific head revision and should offer an explicit refresh.”

## Flagged ambiguities

- “Monaco backend” was corrected to Monaco-based diff presentation; GitHub access and data transformation belong behind the application boundary.
- “PR reviewer” was too narrow: a **Review** may originate from either a **Pull Request** or a **Local Branch Comparison**.
- “Base” can mean the selected **Base Ref** or the computed **Comparison Base**; these are distinct when branches have diverged.
- “Current PR” or “current branch” may move during review; a **Review Snapshot** never does.

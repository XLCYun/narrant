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
head branch refs in one **Repository Entry**.
_Avoid_: Local PR

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

**Review Reader**:
The application view that presents one open **Review Snapshot**, including its
collection of **File Changes** and the freely selected **File Change** content. It
does not assign reading progress or an approval state to File Changes.
_Avoid_: File Changes page, Diff page

**Review Snapshot**:
An immutable **Change Comparison** pinned to exact base, comparison-base, and head
revisions. Its identity and history remain saved even when its Git objects are no
longer available in the user's repository.
_Avoid_: Latest PR, live diff

**Review**:
A durable code-review record owned by one **Repository Entry**. It preserves review
history across application launches, owns one **Review Selection**, and may
contain successive **Review Snapshots** as those refs move.
Opening it activates the runtime resources needed to inspect its current snapshot.
The same **Review** cannot be opened more than once concurrently.
_Avoid_: Review Session, cached review

**Repository Entry**:
A specific locally cloned Git repository registered with Narrant as the home of its
saved review history. Another clone remains a distinct Repository Entry even when both
clones refer to the same remote repository.
_Avoid_: Project, remote repository

**Workspace Trust**:
A reviewer's durable authorization for Narrant to perform normal Git checkout and
run review tooling for one **Repository Entry**.
_Avoid_: Safe repository, Review permission

**GitHub Identity**:
The single GitHub user currently authorized to act through Narrant's GitHub App.
It is an external authorization identity, not a Narrant-owned user account.
_Avoid_: Narrant Account, User Profile

**UI Icon**:
A compact visual symbol that communicates an interface action, object, or status.
It excludes product and third-party logos, application icons, avatars, and
content-provided graphics.
_Avoid_: Logo, brand icon

**Review Workspace**:
The Narrant-owned, disposable checkout of the head revision presented by one
open **Review**. It contains no user-owned state, may be forcibly rebuilt when the
review advances to another snapshot, and is removed when the review closes.
_Avoid_: Repository Entry, user worktree

## Relationships

- A **Pull Request** selects the base and head revisions for one repository-local
  **Change Comparison**
- A **Review Selection** resolves one moving **Base Ref** and **Head Ref** pair
- A **Pull Request Selection** retains its pull-request identity while resolving the
  current refs selected by that pull request
- A **Branch Selection** retains canonical local ref identities
- A **Local Branch Comparison** identifies one repository-local **Change Comparison**
- A **Change Comparison** uses the merge base of its **Base Ref** and **Head Ref** as its **Comparison Base**
- Both **Branch Selections** and **Pull Request Selections** calculate their
  **Comparison Base** from the current base and head commits; a Pull Request does not
  import GitHub's historical Files changed anchor
- A **Change Comparison** contains zero or more **File Changes**
- A **Review Reader** presents one open **Review Snapshot** and one selected **File Change** at a time
- Selecting a **File Change** in the **Review Reader** is navigation only; it does not
  mark that change as viewed, reviewed, or approved
- A **Repository Entry** owns zero or more **Reviews**
- Narrant has at most one current **GitHub Identity**
- A **GitHub Identity** may be represented by cached public profile information while
  GitHub is temporarily unavailable, but signing out removes that representation
- Moving a local clone does not create a new **Repository Entry** when the user
  explicitly relocates its saved path
- A **Repository Entry** may discover pull requests from multiple GitHub repositories
  represented by its Git remotes; it does not own one exclusive GitHub association
- A **Repository Entry** must have **Workspace Trust** before Narrant checks out code or
  starts review tooling for it
- Deleting a **Repository Entry** also deletes its **Workspace Trust**
- A **Review** contains one or more successive **Review Snapshots**
- Historical **Review Snapshots** do not keep their Git objects alive and may become
  unavailable after the user's repository discards those objects
- A **Review** owns exactly one **Review Selection**, while each **Review Snapshot**
  pins the exact commits resolved from that selection at one moment
- A **Review Selection** identity never changes after its **Review** is created; choosing
  another branch pair creates another **Review**
- Within one **Repository Entry**, the same **Review Selection** reopens its existing
  **Review** rather than creating a duplicate
- An open **Review** owns one disposable **Review Workspace** whose repository checkout
  presents the head revision of its current **Review Snapshot**
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
- A recent-review list is an aggregate view of **Reviews** across
  **Repository Entries**

## Example dialogue

> **Dev:** “If new commits are pushed to a **Pull Request** while its **Review** is open, should its **File Changes** update automatically?”
> **Domain expert:** “Not silently—the review presents a specific head revision and should offer an explicit refresh.”

## Flagged ambiguities

- “Monaco backend” was corrected to Monaco-based diff presentation; GitHub access and data transformation belong behind the application boundary.
- “PR reviewer” was too narrow: a **Review** may originate from either a **Pull Request** or a **Local Branch Comparison**.
- “Base” can mean the selected **Base Ref** or the computed **Comparison Base**; these are distinct when branches have diverged.
- “Current PR” or “current branch” may move during review; a **Review Snapshot** never does.

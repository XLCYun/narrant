# Change Review

This context presents changes between a base revision and a head revision as a navigable set of file changes, independent of where those revisions originate.

## Language

**Pull Request**:
A GitHub-hosted **Change Comparison** with collaboration metadata.
_Avoid_: PR data, merge request

**Local Branch Comparison**:
A **Change Comparison** whose base and head revisions come from a local Git repository.
_Avoid_: Local PR, offline PR

**Change Comparison**:
The ordered difference from a comparison base revision to a head revision.
_Avoid_: PR, diff

**Base Ref**:
The branch or revision into which the proposed changes would be integrated.
_Avoid_: Target, old branch

**Head Ref**:
The branch or revision containing the proposed changes under review.
_Avoid_: Source, new branch

**Comparison Base**:
The common ancestor of the **Base Ref** and **Head Ref** from which reviewed changes are calculated.
_Avoid_: Base Ref, base tip

**File Change**:
The before-and-after content and change metadata for one path in a **Change Comparison**.
_Avoid_: Diff file, patch item

**Review Session**:
A review activity that presents one **Review Snapshot** at a time and may explicitly advance to a newer snapshot.
_Avoid_: Workspace, project

**Review Snapshot**:
An immutable **Change Comparison** pinned to exact base, comparison-base, and head revisions.
_Avoid_: Latest PR, live diff

## Relationships

- A **Pull Request** identifies one GitHub-hosted **Change Comparison**
- A **Local Branch Comparison** identifies one repository-local **Change Comparison**
- A **Change Comparison** uses the merge base of its **Base Ref** and **Head Ref** as its **Comparison Base**
- A **Change Comparison** contains zero or more **File Changes**
- A **Review Session** presents exactly one **Review Snapshot** at a time
- Refreshing a **Review Session** replaces its current **Review Snapshot** only after explicit user action

## Example dialogue

> **Dev:** “If new commits are pushed to a **Pull Request** while a **Review Session** is open, should its **File Changes** update automatically?”
> **Domain expert:** “Not silently—the session refers to a specific head revision and should offer an explicit refresh.”

## Flagged ambiguities

- “Monaco backend” was corrected to Monaco-based diff presentation; GitHub access and data transformation belong behind the application boundary.
- “PR reviewer” was too narrow: a **Review Session** may originate from either a **Pull Request** or a **Local Branch Comparison**.
- “Base” can mean the selected **Base Ref** or the computed **Comparison Base**; these are distinct when branches have diverged.
- “Current PR” or “current branch” may move during review; a **Review Snapshot** never does.

---
status: draft
---

# Structure Changes Review View

This specification records the design interview started on 2026-09-12. The preceding
Rust LanguageAnalyzer and shared LSP implementation was merged in PR #181 at
`a010f7150c19e85eca42f40236a835687d43b801`. Product choices below remain pending;
this draft does not authorize an assumed answer to an interview question.

## Requested outcome and settled scope

The user requested a **Structure Changes** tab immediately to the right of **File
Changes** in the Review Reader, using React Flow to visualize the existing Code Graph
Difference. It is another view of the same open Review Snapshot. The user also asked
how to prepare both repository states required by the language analyzers.

Existing decisions remain applicable:

- A Code Graph contains all supported Code Objects in its selected file scope before
  comparison. Deciding which objects to show happens after graph comparison.
- Each node keeps its common category and native kind. Contains and other Code
  Relationships remain distinct; frontend grouping must respect their semantics.
- A graph represents one fixed repository state. `LanguageAnalyzer` accepts a prepared
  repository root and that side's repository-relative source file paths.
- Comparison is `ReviewSnapshot.comparisonBase → ReviewSnapshot.headRevision`, using
  the exact commits already used for File Changes. The selected Base Ref's current tip
  is not the before graph, and moving refs are not resolved again for this view.
- Workspace Trust already covers repository checkout and review tooling. Tool failure
  does not roll back the current Snapshot. These are existing policies, not new prompts.

See [the graph specification](0018-structure-review-code-graph.md),
[the Rust analyzer specification](0019-rust-language-analyzer.md),
[three-dot comparison](../adr/0010-use-current-three-dot-comparisons.md),
[Workspace Trust](../adr/0007-require-workspace-trust.md), and
[analysis readiness](../adr/0012-separate-review-refresh-from-analysis-readiness.md).

## Verified implementation facts

- `src/components/review-reader.tsx` owns the peer tabs. `src/review/types.ts` defines
  ReviewView and the pinned Snapshot; `use-review-lifecycle.ts` retains view and file
  navigation state per open Review.
- The current Review Workspace is only a head checkout at
  `reviews/<review-id>/repo`. `ReviewLifecycle.workspace_path()` verifies Snapshot
  identity when returning the path; it does not hold a lease that prevents a later
  refresh from rebuilding that directory.
- `core_git` already supports detached worktree creation, exact ownership validation,
  failed-creation compensation and removal. Its Git execution should be reused.
- CodeGraph and CodeGraphDiff currently have no frontend serialization, TypeScript
  projection or Tauri command. The graph model should remain independent of React
  Flow positions, dimensions and rendering state.
- A diff lists changes, not every unchanged context node. The frontend therefore needs
  the before graph, after graph and graph difference, plus the pinned Snapshot identity
  and analysis coverage, behind a typed application Interface.
- `CodeNodeData` currently has no source-location contract. NodeId is opaque; parsing
  it in the frontend to manufacture file navigation is invalid. Source navigation was
  deferred by the graph specification and is not added by this request alone.
- Rust methods may be contained by an `impl` Extension and related to a Type by
  `code.member_of`. This does not make the Type their contains parent.
- The current analyzer is synchronous and has request timeouts, but no operation
  cancellation input. Dropping a `spawn_blocking` await does not terminate its running
  rust-analyzer process. Cancellation must reach the running analysis before checkout
  cleanup can be considered safe.

## Proposed deep Module

The frontend asks the Structure Review Module to analyze one `reviewId + snapshotId`.
The backend resolves the saved comparison, verifies Trust and availability, prepares
the two source states, runs the language Adapters, compares the graphs and returns one
Structure Comparison result. Repository paths, Git commands and LSP sequencing remain
behind that Interface. An operation context owns cancellation and progress.

The result contains the pinned comparison identity, both graphs, their difference,
and coverage explaining which changed files were analyzed or unsupported. React Flow
receives a presentation projection of this result. The serialized transport follows
the existing [typed Tauri and application Adapter policy](../adr/0014-use-a-typed-tauri-transport-and-domain-application-adapters.md).

Default file scope is proposed as all changed source files supported by an available
language Adapter in the current Snapshot. Both sides use the same scope policy, with
side-specific existence and paths: before uses `oldPath ?? path` when before content
exists; after uses `path` when after content exists. Additions, deletions and renames
must not produce nonexistent analyzer inputs. Complete repository checkouts supply
semantic context without expanding the graph to every file in the repository.

## Proposed checkout preparation

Prepare two analysis-owned detached worktrees that share the registered Repository's
Git object database. A proposed application-data layout is:

```text
reviews/<review-id>/
  repo/                                  existing head Review Workspace
  structure/<snapshot-id>/<analysis-id>/
    before/                              exact comparison-base commit
    after/                               exact head commit
```

The two extra checkouts hold complete project sources so Cargo manifests, module
files and build inputs remain available. They are not independent clones and are not
checked out in the user's development directory. Their lifetime belongs to one
analysis attempt; an attempt identifier prevents a cancelled attempt's cleanup from
removing the files of a later retry.

Reusing the existing head `repo/` would save one checkout, but currently requires a
new lease across analysis and refresh. Holding the Review lifecycle lock during LSP
startup would also couple optional analysis latency to ordinary review operations.
Dedicated before/after paths let the current Review refresh without modifying the
sources being analyzed. This is the recommended trade-off, pending the final shared
design confirmation.

Preparation and release must be owned by one Module:

- Verify the Review is open, the requested Snapshot is current and its Repository is
  trusted before materialization. Use the Snapshot's saved commit identities.
- Create each checkout through Core Git. If the second fails, compensate the first.
- Stop the analysis and its language-server/build processes before removing checkouts.
  Success, error, cancellation, refresh, Repository deletion and application exit all
  need an explicit release path.
- Check precise application path, Repository Git common directory and linked-worktree
  registration before removal. Never broadly prune unrelated worktrees.
- Record enough ownership information to recover a process crash and retry cleanup;
  a temporary-directory destructor alone does not remove Git worktree registrations.
- Keep successful graph results in process memory under Snapshot identity, file scope
  and analyzer configuration. Tab switching can reuse a completed result. A refreshed
  Snapshot must never display a late result from its predecessor.
- Release the analysis checkouts after completion. Reusable tool state, if retained,
  belongs outside these disposable source directories; persistent graph storage and
  long-lived two-sided checkouts are not assumed.

This preserves the existing meaning and lifetime of Review Workspace. If this
trade-off is accepted, record the analysis-checkout decision separately because it
affects refresh concurrency, disk use and crash recovery.

## React Flow projection facts

React Flow supports nested nodes through `parentId`, with child positions relative to
their parent; parent nodes precede children in the input. It permits relationships
crossing those groups. Parent dimensions and automatic layout are application work.
[Official sub-flow documentation](https://reactflow.dev/learn/layouting/sub-flows).

React Flow does not include automatic layout. Its documentation notes Dagre's limitation
for sub-flows connected to outside nodes; ELK supports compound layout and edge routing.
An ELK integration is a candidate Implementation choice to validate with the real graph,
not a product decision for the user. [Official layout documentation](https://reactflow.dev/learn/layouting/layouting).

`contains` supplies nesting; references supply typed edges. Own-content status and
contains/reference changes must remain distinguishable: a parent whose child changed
does not thereby have modified own code. If Q1 selects a combined graph, the projection
must handle changed containment without blindly unioning two parent trees into a
multi-parent or cyclic visual hierarchy.

## Design tree and first frontier

| Decision | Recommendation | Status |
| --- | --- | --- |
| Peer tab and library | Structure Changes immediately after File Changes; React Flow | Explicit user request |
| Comparison identity | Existing Snapshot's comparison-base and head commits | Existing contract |
| Checkout strategy | Two attempt-owned detached worktrees, release after analysis | Proposal; facts verified |
| Q1: presentation | One combined difference graph with explicit added/removed/modified states | Awaiting answer |
| Q2: initial visible objects | Changed objects and their containing context; allow expansion of unchanged objects | Awaiting answer |
| Q3: first language coverage | Existing Rust Adapter; explicitly show unsupported changed-file coverage | Awaiting answer |

Q1 asks combined versus side-by-side before/after graphs. Q2 changes the initial
presentation only, not the complete extraction inside the chosen file scope. Q3 asks
whether other language implementations belong in this slice.

After these answers, recompute the frontier for any genuinely unsettled product
choices, particularly scope controls and interactions affected by the chosen view.
Do not reopen settled comparison, Trust or graph extraction contracts as questions.

## Acceptance outline to refine after the interview

- The Structure Changes tab preserves the open Review and Snapshot, respects existing
  Review navigation, and uses React Flow to display containment and typed relationships.
- Real before/after repository states generate the displayed result through the existing
  LanguageAnalyzer and graph comparison Interfaces.
- Fixtures cover body-only modification, added/removed nodes and edges, relationship-only
  parent changes, nesting, cross-group references and unchanged contextual objects.
- Loading, error, retry, cancellation, no structural differences and unsupported-only
  scope are distinguishable. Missing analysis is never displayed as a proven empty diff.
- Refresh during analysis, stale completion, failure during the second checkout and
  cleanup after process interruption are verified at the application Interface.
- Reuse the Preview Harness for wide/narrow windows, long names, nested objects, cross-
  group edges and meaningful interactions. Verify actual browser rendering in addition
  to frontend tests, type checking, lint and production build.

No implementation ticket or new PR has been created during this first design round.

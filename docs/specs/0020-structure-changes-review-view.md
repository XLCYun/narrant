---
status: accepted
---

# Structure Changes Review View

This specification records the design interview started on 2026-09-12. The preceding
Rust LanguageAnalyzer and shared LSP implementation was merged in PR #181 at
`a010f7150c19e85eca42f40236a835687d43b801`. The user accepted Q1–Q3 and required reuse
of the existing head checkout on 2026-09-12. The design now proceeds to implementation.

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

## Application Module

The frontend asks the Structure Review Module to analyze one `reviewId + snapshotId`.
The backend resolves the saved comparison, verifies Trust and availability, prepares
the two source states, runs the language Adapters, compares the graphs and returns one
Structure Comparison result. Repository paths, Git commands and LSP sequencing remain
behind that Interface. An operation context provides cancellation.

The result contains the pinned comparison identity, both graphs, their difference,
and coverage explaining which changed files were analyzed or unsupported. React Flow
receives a presentation projection of this result. The serialized transport follows
the existing [typed Tauri and application Adapter policy](../adr/0014-use-a-typed-tauri-transport-and-domain-application-adapters.md).

Default file scope is all changed source files supported by an available
language Adapter in the current Snapshot. Both sides use the same scope policy, with
side-specific existence and paths: before uses `oldPath ?? path` when before content
exists; after uses `path` when after content exists. Additions, deletions and renames
must not produce nonexistent analyzer inputs. Complete repository checkouts supply
semantic context without expanding the graph to every file in the repository.

## Checkout preparation: reuse head

Reuse the Review's existing head checkout. Create only one analysis-owned detached
worktree at the exact comparison-base commit, sharing the Repository's Git object
database. The application-data layout is:

```text
reviews/<review-id>/
  repo/                                  existing head, used under a Snapshot lease
  before/                                exact comparison-base commit
```

Both sides expose complete project sources so Cargo manifests, module files and build
inputs remain available. The additional before checkout belongs to one analysis attempt;
the Review lease serializes actual workers, so an old worker finishes cleanup before a retry can acquire the same path. No second
head checkout or independent clone is created.

The Review Lifecycle exposes `run_in_workspace` for synchronous work in a fixed
Snapshot. The callback receives the comparison, paths and a bound operation
context. Lifecycle starts the blocking worker and keeps its private lease there;
the caller no longer acquires or transfers a lease or composes revocation flags. It
verifies Review/Snapshot identity, Trust and the expected head while establishing the
protection. Refresh and close revoke active analysis, wait for its worker and language
server processes to stop, and only then mutate or remove head. Releasing the use does
not remove head; normal Review lifecycle remains its owner. A bare path lookup is not
sufficient, and cancelling only the frontend await is not sufficient.

Before executing the callback and before returning success, validate the Repository's
Git common directory, exact canonical worktree registration, Snapshot HEAD and tracked
contents relative to HEAD. Each validation uses one fresh `worktree list --porcelain -z`
result for both path and HEAD, followed by `git diff --quiet HEAD --`. The checks do not
cover untracked files or edits restored between the two checks, and do not provide
filesystem snapshot isolation. The lease coordinates application-managed mutations;
it does not replace either check against external changes.

Review Lifecycle owns admission, cancellation binding, locking and worker lifetime.
Structure Changes owns before preparation, two-side analysis and before cleanup inside
that execution range:

- Verify the Review is open, the requested Snapshot is current and its Repository is
  trusted before materialization. Use the Snapshot's saved commit identities.
- Create the before checkout through Core Git; failure releases any partial before
  state and head usage without deleting the existing head checkout.
- Stop the analysis and its language-server/build processes before removing before
  or permitting head to change.
  Success, error, cancellation, refresh and application exit all use this release path.
  A future Repository deletion operation must drain these uses before deleting its workspaces.
- Check precise application path, Repository Git common directory and linked-worktree
  registration before removal. Never broadly prune unrelated worktrees.
- Record enough ownership information to recover a process crash and retry cleanup;
  a temporary-directory destructor alone does not remove Git worktree registrations.
- Keep successful graph results in process memory under Snapshot identity, file scope
  and analyzer configuration. Tab switching can reuse a completed result. A refreshed
  Snapshot must never display a late result from its predecessor.
- Release the before checkout after completion. Reusable tool state, if retained,
  belongs outside these disposable source directories; persistent graph storage and
  an additional long-lived checkout is not assumed.

This preserves the existing meaning and lifetime of Review Workspace and implements
the user's explicit instruction not to repeat the head checkout. See [ADR-0031](../adr/0031-reuse-review-head-for-structure-analysis.md).

### Parallel sides and dependency settings (2026-09-14)

Inside the fixed-workspace callback, run two independent sides concurrently. Before
prepares its checkout and immediately analyzes it; after immediately analyzes the
existing head. Preparation, project loading and semantic queries can overlap. An empty
side produces an empty graph without starting a server or preparing an unnecessary
checkout. No second head checkout is introduced.

Both workers use the same frozen test-code option and Rust dependency scope. A failure
or panic on either side cancels its peer. Cancellation, close, refresh and abandoned
requests wait for both actual workers to stop before releasing the lease; before
cleanup belongs to its worker, and cleanup failures remain visible together with any
original analysis error or panic. Explicit cancellation stays distinct from real
failures. Only after both graphs succeed does the callback compare/project the graphs;
Lifecycle then validates head before publishing success.

Application Settings adds **Structure Changes / 结构改动**, following the existing settings
navigation and card layout. A language selector currently offers Rust. Its three
persisted options are repository-only, repository plus standard library (default),
and repository plus standard library and third-party dependencies. Saving takes effect
for the next analysis; a completed graph offers **Analyze again / 重新分析**. A settings
change does not mutate an already completed result or change an in-flight analysis.
Saving failure retains the last saved selection and permits retry.

See the [adapter scope contract](0019-rust-language-analyzer.md#configurable-dependency-scope-2026-09-14)
and [PR31 serial/parallel benchmark](../research/structure-review-settings-parallel.md).

## React Flow projection facts

Library and integration-test crate roots supplied by the Rust analyzer remain
separate container cards. Root-level functions belong to their respective cards;
they must not be collected into one unowned function list. The root labels are the
repository-relative entry paths supplied by the analyzer. The representative
`structure-crate-roots` preview covers this grouping, long test signatures and
cross-crate calls without changing production review data.

React Flow supports nested nodes through `parentId`, with child positions relative to
their parent; parent nodes precede children in the input. It permits relationships
crossing those groups. Parent dimensions and automatic layout are application work.
[Official sub-flow documentation](https://reactflow.dev/learn/layouting/sub-flows).

React Flow does not include automatic layout. Its documentation notes Dagre's limitation
for sub-flows connected to outside nodes; ELK supports compound layout and edge routing.
The Implementation uses ELK for compound layout and routing in a local Web Worker. [Official layout documentation](https://reactflow.dev/learn/layouting/layouting).

`contains` supplies the declaration hierarchy; semantic ownership can additionally
group members in a type card as described below. Own-content status and
contains/reference changes remain distinguishable: a parent whose child changed does
not thereby have modified own code. The combined projection handles changed
containment without unioning two parent trees into a multi-parent or cyclic hierarchy.

## Accepted design tree

| Decision | Recommendation | Status |
| --- | --- | --- |
| Peer tab and library | Structure Changes immediately after File Changes; React Flow | Explicit user request |
| Comparison identity | Existing Snapshot's comparison-base and head commits | Existing contract |
| Checkout strategy | Reuse existing head under lifecycle protection; create only before | Explicit user correction |
| Q1: presentation | One combined difference graph with explicit added/removed/modified states | Accepted |
| Q2: initial visible objects | Changed objects and their containing context; allow expansion of unchanged objects | Accepted |
| Q3: first language coverage | Existing Rust Adapter; retain coverage data to distinguish unsupported-only results | Accepted; coverage details UI removed by user follow-up |

Q1 chooses a combined graph. Q2 changes the initial
presentation only, not the complete extraction inside the chosen file scope. Q3 limits
the first implementation to the existing Rust Adapter.

The first entry to this tab requests analysis lazily. A completed result is reused for
the same Snapshot and analysis options; the page offers retry after failure and cancellation while running.
Selecting a node displays its declaration/content details rather than inventing source
navigation from its opaque ID. Layout is read-only, with pan, zoom, fit and context
expansion. These defaults implement the accepted scope without additional language
implementations, persistent graph storage or a new file-scope picker.

### Test-code analysis switch

The Structure Changes toolbar provides an **Include test code / 分析测试代码** switch,
off by default. It changes the common Code Graph extraction options and requests a
new comparison using the same option on both sides. It is available for ready, empty,
loading, failed and cancelled states. Switching cancels an older in-flight request;
late results for the old scope cannot replace the new one. Retry preserves the chosen
option. A new Snapshot starts with the default scope, and cached results from a
different option are never reused. The existing head checkout is still reused.

The Rust policy excludes declarations spelled `#[test]` without checking macro identity,
and proven test-only cfg scopes, including module inheritance; it does not classify tests
from filenames or Cargo targets. See the
[Rust scope](0019-rust-language-analyzer.md#test-code-analysis-scope).
This is an analysis option, not a display filter. The `structure-tests-only` preview
starts with an empty diff and restores a modified test when the switch is enabled.

### Persistent details sidebar

Completed comparisons use the same shadcn resizable split as File Changes: a
persistent details sidebar on the left and the graph on the right. The sidebar
starts at 320 pixels, can be resized with the pointer or keyboard, and retains its
pixel width during window resizing within a 180-pixel minimum and 55% maximum. The
graph retains at least 45% of the split. Selecting a different object does not reset
the split width or recreate the graph viewport.

Before selection, the sidebar prompts the user to choose an object. Selecting a
card or member updates the sidebar and highlights that exact object. One read-only
Monaco inline diff compares the before/after `source` excerpts, combining declaration
and implementation with their original comments, whitespace and nested members.
The editor shares File Changes' light/dark themes, wraps long lines for display only,
and grows for short excerpts up to a bounded height with independent scrolling.
An added/removed object has an empty original/modified side. A present node without
source shows an unavailable message rather than a fabricated addition or deletion.
No formatter reconstructs source from the normalized signature and implementation.
These display excerpts do not participate in structural comparison: comment-only or
child changes in a parent's excerpt do not make its own graph content modified.
Source is retained in the comparison before the temporary checkout is cleaned up.
The sidebar scrolls independently; selecting another object starts its
details at the top. A new Snapshot clears the previous selection.
The existing object/reference display filters apply to sidebar details as well.

The toolbar contains the test-code switch, reanalysis and display options, aligned
to the right. Object counters and the file-coverage button and details mode are
removed. Coverage data remains available to distinguish unsupported-only results
from an analyzed empty graph. The sidebar header displays only its title, without
an explanatory sentence.

### Show relationships for the selected object

The graph initially hides all relationship lines and their connection markers.
Selecting an object reveals only incident lines to that exact Code Object, in both
directions. Original relationship endpoints determine identity, so selecting a method in a class card
does not reveal the other methods' lines. Calls between members of one card,
recursive calls and removed relationships follow the same rule.

Field type relationships remain eligible even when unchanged, and their graph-local
targets are prepared as one-hop context before selection. Other relationships retain
the unchanged-relationship and import controls; enabling those controls without a
selection does not reveal lines. Only
endpoints represented in the displayed graph are eligible. External and Unresolved
targets remain hidden. Contains continues to define the nested layout, while a
former contains edge, when present, follows the same selection rule as other lines.

Clicking blank canvas clears selection and hides lines again. Switching Snapshots
also leaves no object-specific lines visible. Selecting objects
changes only line/marker visibility and sidebar content: it does not rerun analysis,
rearrange objects or reset the viewport. Candidate routes remain in the layout so
revealing a relationship is immediate and does not move its endpoints.

### Collapse containers and types

Containers and Types with members currently included in the view have a separate
collapse button. The button always selects that same Code Object for the sidebar
and toggles only its own collapsed state. The name remains a separate selection
button. Collapsing a parent preserves its descendants' individual collapse choices.
Ordinary member rows have no additional collapse control.

A collapsed card keeps its header, a count of included descendants (or fields and
methods for a Type), and the count of changed descendants. The latter is separate
from the object's own change status. ELK first lays out the current display scope
fully expanded. Collapse synchronously hides descendant cards and member rows and
shrinks only the folded card inside its original bounds. Other cards and their
parents keep the initial coordinates and sizes, reserving space for expansion.
It does not change graph
membership, comparison counts, or the set of unchanged objects revealed by the
existing context controls. The display menu offers Collapse all and Expand all;
these affect every collapsible object already included in the current view,
including descendants hidden under collapsed parents. Collapse all clears selection.
State is scoped to the current comparison; a new Snapshot starts expanded.

Candidate relationships retain their original Code Object endpoints. If an endpoint
is hidden by collapse, its visible collapsed ancestor provides the display port.
Only relationships incident to the exact selected object appear; selecting a
collapsed parent does not select all its descendants' relationships. Relationships
entirely inside one collapsed card are hidden. A proxied endpoint's tooltip contains
only its readable object name, qualified by the containing Type for a member (for
example `Repository::load`), without a target or module-description prefix.
External and Unresolved references remain hidden.

Folding and unfolding project the cached expanded layout without running ELK,
showing a layout-loading overlay, adjusting the viewport or remounting React Flow.
Existing edge routes are retained whenever their port positions stay the same;
only a moved endpoint uses a local orthogonal connection to its collapsed card.
Expanding restores the original positions, sizes, member ports and routed paths.
Changing display filters or revealing additional context still computes a new
expanded layout for the new display scope.

### Compact class-diagram presentation

The user requested class-diagram-style cards after finding the individual object
boxes too spacious. A visible Type now has a name/header followed by compact
attribute and method sections. Each ordinary field or callable occupies a 30-pixel
row with its source signature, change marker and a button opening that exact
object's before/after details. Long signatures truncate in the diagram and remain
fully available in the title and details. Type status still describes that Type's
own difference, independently of its members' statuses.

An Extension with one unambiguous `code.applies_to` target that is a Type in the
graph can be grouped with that Type. Rust `impl` declarations become clickable
subsection rows, retaining their signature, status and method grouping. Explicit
`code.member_of` can also place a member with its Type. Surviving objects use the
after graph's ownership, while removed objects use before; the projection never
infers ownership from names or opaque IDs. Ambiguous, unresolved or graph-external
ownership keeps the implementation separate. A candidate that would introduce
a visual containment cycle is rejected.

Declarations that contain nested named objects retain their container and nested
cards, so compacting methods does not discard local functions or nested types.
Free functions and data declarations use compact lists inside their visible module;
root declarations share a presentation-only list without acquiring a fake module
identity. Modules retain their nesting. Counts refer to Code Objects, including
member rows, rather than the smaller number of React Flow cards. Existing visibility
filters and expansion remain available, including expanding an implementation's
methods from its Type card.

Each member has distinct incoming/outgoing handles at its row, so grouping does not
merge method calls into class-level relationships. Calls within one card, recursive
calls, removed placement and former implementation-target relationships remain
representable. Ownership already expressed by the containing card is not drawn as
a duplicate edge; changes to displayed relationships remain in object details.
The graph contract and Rust extraction are unchanged by this presentation policy.

ELK arranges cards while member ports have fixed positions. React Flow connects
`sourceHandle` and `targetHandle` to those exact members. See the
[React Flow handle documentation](https://reactflow.dev/learn/customization/handles)
and [ELK port constraints](https://eclipse.dev/elk/reference/options/org-eclipse-elk-portConstraints.html).

### Reference display policy

Only resolved targets represented by a node in the graph are displayed. All External
and Unresolved targets are always omitted, along with their incident edges, whether
they refer to repository code outside the selected graph, standard-library code,
third-party code or symbols without scope descriptions. There is no control to reveal
these targets. The Display options menu has three controls: all objects, unchanged
relationships and imports. These controls apply only to displayed graph nodes and
their relationships.

Canvas layout, object counts, change markers, expansion and both relationship lists
in object details use the same projection. An object whose only differences are
hidden relationships is unchanged in this projection; its own declaration or code
changes still count. If no visible changes remain, the view directs the user to File
Changes. Original code shown in details is preserved, including any external names
appearing in that code.

The Rust policy omits `code.reads` and type relationships outside field declarations.
Fields emit `code.type_use` only for graph-local Type targets, as specified in
[graph-local field type relationships](#graph-local-field-type-relationships).
There is no separate type-use display checkbox. Signatures and source code still
include other type annotations and value reads, and structural relationships such as
implementation and membership remain available.

Complete backend graphs, graph differences and `externalSymbols` remain intact.
The frontend does not decode opaque IDs or infer scope from their format. Ownership
checks still inspect the complete references: hiding an External or Unresolved
candidate must not turn ambiguous ownership into a seemingly unique graph-node
target. The import toggle does not change the changed-object count.

Rust `use` Extension nodes, including `pub use`, are always omitted from this view,
including Show all objects, expansion and Show imports. Their incident relationships
and import-only targets do not enter the layout. Object totals, changed-object counts,
expansion hints and relationship details apply the same projection. A parent whose
only relationship changes involve hidden `use` nodes is unchanged in the projection.
An otherwise empty graph explains that source changes can still be read in File
Changes. The complete analyzer graphs still retain import declarations and their
differences. Actual calls on displayed objects remain available.

Rust closures no longer appear as separate Callable or ExecutionRegion nodes. Their
content changes and outgoing calls remain on the nearest modeled declaration. This
is an analyzer extraction policy, not a frontend filter; both complete input graphs
follow it before computing the difference.

## Acceptance criteria

- The Structure Changes tab preserves the open Review and Snapshot, respects existing
  Review navigation, and uses React Flow to display containment and typed relationships.
- Real before/after repository states generate the displayed result through the existing
  LanguageAnalyzer and graph comparison Interfaces.
- Fixtures cover body-only modification, added/removed nodes and edges, relationship-only
  parent changes, nesting, cross-group references and unchanged contextual objects.
- Loading, error, retry, cancellation, no structural differences and unsupported-only
  scope are distinguishable. Missing analysis is never displayed as a proven empty diff.
- Refresh during analysis, stale completion, before-checkout creation failure and
  cleanup after process interruption are verified at the application Interface.
- Reuse the Preview Harness for wide/narrow windows, long names, nested objects, cross-
  group edges and meaningful interactions. Verify actual browser rendering in addition
  to frontend tests, type checking, lint and production build.

## Implementation and local verification

- Backend entry: `structure_review::StructureReview::analyze`. The actual blocking
  worker owns the Snapshot lease; dropping its async waiter revokes its linked
  cancellation signal without releasing the head early. Common LSP reads/writes
  observe cancellation at bounded intervals and process cleanup retains ownership.
- Transport: `analyze_structure_review` returns both complete graphs, the existing
  graph difference and per-side coverage. Frontend `StructureReviewAdapter` keeps
  Tauri calls outside the view; `useStructureReview` caches one result per open Review's
  current Snapshot and rejects stale results.
- Presentation: React Flow uses after containment for surviving objects and before
  containment for deleted objects. Removed containment that cannot be expressed by
  the combined tree remains an explicit edge. Expansion does not change analysis scope.
- Local tool configuration: `NARRANT_RUST_ANALYZER` overrides the development default
  `<build-repository>/.tools/rust-analyzer` (`.exe` on Windows). Automatic installation
  and packaged tool distribution remain deferred by specification 0019. Missing or
  failed language tooling produces a retryable analysis failure, never an empty diff.
- Automated acceptance covers one additional checkout, exact merge-base, side-specific
  additions/deletions, body changes, stale Snapshot rejection, cleanup failures,
  refresh/close revocation, abandoned async waiters and crash recovery. The ignored
  `real_server_builds_review_structure_comparison` test exercises the application
  Interface with a real rust-analyzer process and real Git worktrees.
- The generic Preview Harness contains structure graph, loading/cancellation, failure/
  retry, no-change and unsupported-only scenarios. Representative data includes
  nested `impl` objects, long names and relationships across containing groups.

### Verification on 2026-09-12

- Rust full suite: 551 passed, 3 opt-in language-server tests ignored. The new real
  rust-analyzer application test was then run separately and passed (3 before nodes,
  3 after nodes, 5 changes, including body-only change and declarations added/removed).
- Frontend full suite: 41 files, 337 tests passed with two workers. Type checking,
  ESLint, Rustfmt and Clippy with warnings denied passed. The production frontend
  build passed; Vite retains its large-chunk advisory for Monaco and the lazy graph view.
- In-app browser checks used explicit 1440×1000, 900×800 and 520×800 viewport overrides
  (reported CSS viewports were 1309×909, 818×727 and 472×727 at the existing browser zoom).
  Tabs and toolbars stayed within the viewport. Long names truncated in graph cards,
  wrapped in details, and code scrolled within its own panel. The narrow details panel
  had no horizontal overflow. The temporary viewport override was reset after testing.
- Browser interactions verified context expansion, all objects, unchanged relationships
  including `code.member_of`, node details, per-file coverage, loading/cancellation/retry,
  failure, no structural changes and unsupported-only coverage in light and dark themes.

### Reference display follow-up verification on 2026-09-12

- Rust full library suite: 555 passed, 4 real-server tests opt-in. The new real
  rust-analyzer test passed separately, confirming a readable `Arc<T>::new` dependency
  and an unselected repository `helper` target with distinct scopes.
- Frontend: 341 tests passed across the full run and a focused rerun of the release
  preparation test after restoring Cargo to PATH. TypeScript, ESLint, Rustfmt, Clippy
  with warnings denied and the production frontend build passed.
- The in-app preview request remained queued in this session. Independent Chrome
  rendered the same Preview Harness at 1440×1000, 900×800 and 520×800 instead. Ten
  screenshot/accessibility-tree captures and element measurements verified light/dark
  themes, the English menu's wrapped label, long-name details, independent filter
  toggles and names in graph cards and details. No browser exceptions or horizontal
  overflow were found in the toolbar, menu or details panel. Viewport overrides were
  cleared after checking. In-app interaction verification of these follow-up controls
  remains pending.

### Import declaration display follow-up verification

- Fourteen focused graph-presentation and analysis-lifecycle tests passed, including
  added/removed/unchanged `use` nodes, all display controls, parent-only import changes,
  preserved type-use edges, consistent details and unchanged complete input graphs.
- TypeScript, production build and repository ESLint passed. The 1168 bundled icons
  matched the generated source assets. ESLint excluded the ignored `.tools` and
  `.spikes` directories containing local tools and Cargo sources.
- The in-app preview request remained queued. Independent Chrome checked the same
  Preview Harness at 1440×1000, 900×800 and 520×800 with light and dark themes. Eighteen
  screenshots and accessibility trees covered defaults, expansion, all display
  options enabled, parent details, long-name details, import-only empty state and
  unchanged parent context. Geometry checks found no toolbar, menu or detail overflow;
  the browser reported no exceptions. The viewport override was cleared afterward.

### Type-use display follow-up verification

This records the earlier display-only behavior, superseded by removing type-use and
read extraction in the current Rust adapter.

- Fifteen focused graph-presentation and analysis-lifecycle tests passed. The type-use
  filter hides edges and targets introduced only by those edges, applies to both
  details lists, preserves calls and complete comparison data, and can be enabled
  independently of other display controls.
- TypeScript, repository ESLint (excluding ignored local tools/caches) and the
  production frontend build passed. No application installer was built.
- The in-app request remained queued. Independent Chrome verified the Preview Harness
  at 1440×1000, 900×800 and 520×800 in light/dark themes. Sixteen screenshot and
  accessibility captures checked default visibility, other enabled controls, the new
  checkbox, graph/details toggling and the English narrow menu. Geometry checks found
  no toolbar/menu/details overflow or browser exceptions; viewport overrides were reset.

### Relationship extraction follow-up verification on 2026-09-13

- Fourteen focused graph-presentation and analysis-lifecycle tests passed. The preview
  fixture now matches the Rust adapter's omission of type-use and read relationships;
  calls, signatures and the four remaining display controls are preserved.
- TypeScript, repository ESLint (excluding ignored local tools/caches) and the
  production frontend build passed. No application installer was built.
- The in-app preview request remained queued. Independent Chrome verified the same
  Preview Harness at 1440×1000, 900×800 and 520×800 in light/dark themes. Thirteen
  screenshots, accessibility trees and geometry checks covered defaults, all remaining
  controls, object details with a long signature and the English narrow menu. No
  removed relationship or exclusive target appeared, including with all controls on.
  Toolbar/menu/details had no horizontal overflow or browser exceptions; viewport
  overrides were reset. In-app interaction verification remains pending.

### Class-diagram follow-up verification

- The full frontend suite passed: 41 files, 350 tests. Public presentation tests
  cover compact attributes/methods, per-member ports, calls within one card and
  recursive calls, changed type/implementation ownership, ambiguous ownership,
  nested declarations, root function lists, expansion and unchanged analysis data.
  A class with four properties and eight methods occupies one card below 500 pixels
  high, while all thirteen Code Objects retain their identities.
- TypeScript, repository ESLint (excluding ignored local tools/caches) and the
  production frontend build passed. Rust extraction and transport were not changed;
  this follow-up did not rebuild the application installer.
- The in-app preview request remained queued. Independent Chrome checked the same
  Preview Harness at 1440×1000, 900×800 and 520×800 in light/dark themes, with 28
  screenshot/accessibility/geometry captures. Checks cover default and expanded
  members, all display options, dense multi-implementation cards, long signatures,
  separate method/field/implementation details and the English menu. Member buttons
  stay inside each row, ports align with row centers, and routed edges do not cross
  unrelated leaf-card interiors in these fixtures. No browser exceptions or toolbar,
  menu or details overflow were found; viewport overrides were cleared afterward.
  In-app interaction verification remains pending.

### Crate-root follow-up verification

- Public graph presentation and preview routing checks pass (21 tests), including
  separate library/test cards, unchanged root content with a modified member,
  cross-crate member ports and original-object details. TypeScript, ESLint and the
  production frontend build pass.
- Independent Chrome verifies `structure-crate-roots` at 1440×1000, 900×800 and
  520×800, in light/dark themes. Nine screenshot/accessibility/geometry captures
  cover card ownership, long-signature details and the options menu. There is no
  horizontal overflow or browser exception; viewport overrides are cleared. The
  in-app browser request remains queued, so its interaction verification is pending.

### Graph-local target display follow-up verification

- The full frontend suite passes: 41 files, 354 tests. Focused coverage checks every
  display-option combination, repository/dependency/unknown External scopes,
  Unresolved targets, removed references, hidden-only relationship changes, retained
  local calls, consistent details and preservation of the complete comparison.
  Ambiguous ownership remains ambiguous when one candidate is hidden.
- TypeScript, repository ESLint (excluding ignored local tools/caches) and the
  production frontend build pass. The existing large-chunk warning remains. This
  display change does not modify Rust analysis or rebuild the application installer.
- Independent Chrome completes 19 screenshot/accessibility/geometry captures at
  1440×1000, 900×800 and 520×800 in light/dark themes. Checks cover the three remaining
  display controls, expansion, local relationship details, long signatures, retained
  original code, hidden-only empty state and unchanged context, and the English menu.
  No floating target, hidden relationship, horizontal overflow or browser exception
  appears; viewport overrides are cleared afterward. The in-app preview request
  remains queued, so in-app interaction verification is still pending.

### Persistent sidebar follow-up verification

- The full frontend suite passes: 42 files, 357 tests. New view tests exercise the
  public StructureChangesView with real graph projection and node interactions,
  substituting only React Flow's browser drawing layer in jsdom. They cover the
  permanent sidebar, added/removed/type selection, exact selection highlighting,
  canvas lifetime, coverage switching, hidden-reference filtering, preserved own
  code, a new Snapshot clearing selection and an empty visible comparison.
- TypeScript, repository ESLint (excluding ignored local tools/caches) and the
  production frontend build pass. The existing large-chunk warning remains. No
  Rust analysis or transport changed; this follow-up does not rebuild an installer.
- Independent Chrome completes 19 screenshot/accessibility/geometry captures at
  1440×1000, 900×800 and 520×800 in light/dark themes. Pointer and keyboard resizing,
  pixel-width retention across window resizing, unchanged graph viewport during
  selection, sidebar scroll reset, coverage, long content, all display options,
  hidden-only empty state and the English menu pass. There are no modal remnants,
  horizontal overflow, hidden targets or browser exceptions. Viewport overrides are
  cleared afterward. The in-app request remains queued; in-app interaction
  verification is pending.

### Selected-object relationship follow-up verification

- The full frontend suite passes: 42 files, 358 tests. View checks cover no initial
  lines, incoming/outgoing and removed relationships, exact member identity within
  one class card, recursive calls, coverage/blank-canvas clearing, retained sidebar
  behavior and stable canvas lifetime.
- TypeScript, repository ESLint (excluding ignored local tools/caches) and the
  production frontend build pass. The existing large-chunk warning remains. This
  change does not modify Rust analysis or rebuild the installer.
- Independent Chrome completes 20 screenshot/accessibility/geometry captures at
  1440×1000, 900×800 and 520×800 with light/dark themes. Default/cleared graphs have
  no relationship lines or connection markers; selected functions and methods show
  only their eligible incident lines, even inside the same class card. All display
  controls remain subject to selection, and object positions and viewport transforms
  stay unchanged while selecting. English controls, sidebar content and hidden-target
  filtering pass without horizontal overflow or browser exceptions. Viewport overrides
  are cleared afterward. The in-app request remains queued; its interaction
  verification is pending.


### Test-code switch follow-up verification

- The full frontend suite passes: 42 files, 363 tests. Coverage includes the default
  off option at the transport boundary, matching options on reanalysis and retry,
  cancellation and rejection of late results, hidden-tab laziness and a new Snapshot
  resetting the scope. View tests cover the switch in all states and a tests-only
  empty comparison restoring distinct before/after code when enabled. After strengthening
  the Snapshot-reset regression, the 19 related tests pass in a final sequential run.
  One unchanged view test exceeded its 5-second limit during concurrent dependency
  compilation; its sequential rerun passes.
- TypeScript, production frontend build and project-source ESLint pass. Plain
  `pnpm lint` also traversed ignored local `.tools` caches and reported errors there;
  the source check excludes that directory. Vite retains its existing large-chunk
  warning.
- Independent Chrome completes 15 screenshot/accessibility/geometry captures at
  1440×1000, 900×800 and 520×800 with light/dark themes. Real pointer and keyboard
  interactions cover the default empty graph, inclusion, selected details, exclusion
  again, loading cancellation, retry after cancellation/failure and the narrow English
  menu. There is no horizontal overflow or browser exception, the switch has its
  accessible name and viewport overrides are cleared afterward. The in-app preview
  request remains queued and in-app interaction tooling is unavailable in this session;
  in-app interaction verification remains pending.

## Graph-local field type relationships

Status: accepted. Both scope and unchanged-relationship behavior were confirmed by
the user before implementation.

### Requested behavior and settled meaning

Selecting the `PullRequestGroup.pull_requests` field whose declaration is
`Vec<DiscoveredPullRequest>` should show a relationship from that field row to the
`DiscoveredPullRequest` Type card when the target belongs to the Code Graph. Targets
outside the Code Graph, including standard-library wrappers, dependencies, unselected
repository declarations and unresolved symbols, are not displayed. A relationship
does not expand the selected file scope or create nodes for dependency declarations.

The relationship belongs to the field Code Object, not to its containing Type.
Containment and the exact member-row ports keep their existing meanings. Membership
in a Code Graph is distinct from visibility in the current Review View; an unchanged
or currently omitted Type can still belong to the analyzed graph.

### Extraction and display policy

- Both field Value nodes and Type nodes already exist. Rust collects type positions
  from record and tuple field declarations, covering struct, enum-variant and union
  fields. Ordinary function signatures, type-alias definitions, bounds and local/body
  type uses remain omitted. Existing source-member and target-Type ports render the
  resulting relationship without changing object identity or containment.
- Reuse `CodeReference` with semantic `code.type_use` and a
  `ReferenceTarget::Node`. No shared graph or transport schema extension is needed.
  Each side emits its own relationships before the existing graph comparison runs.
  Changing `Vec<A>` to `Vec<B>` removes the relationship to A and adds one to B when
  those targets belong to their respective graphs.
- Traverse field-type syntax structurally, including nested generic arguments and
  inner reference, pointer, array, slice, tuple and function-pointer types. A graph-external
  wrapper must not stop traversal into its arguments. Do not scan signature strings
  or infer target identity from display names.
- Use the existing LSP definition operation to identify actual declarations, then
  restrict these type targets to retained Type nodes in the selected documents.
  Reject other target URIs before the existing graph-external document loader, so this
  new path does not read and parse dependency source merely to discard its edge.
  LSP lookups still have a cost; graph-local output does not imply graph-local server
  analysis. Existing call and module-ownership queries keep their current behavior.
- The alias rule follows the declaration actually named: `field: Alias`
  points to an in-graph Alias Type, without recursively substituting the alias's
  underlying type. Lifetimes, const-generic execution expressions and local generic
  parameters do not become extra Type nodes. Unsupported or unresolved targets do
  not create guessed edges.
- Changed references introduce their graph-local targets as visible context. Type
  references from displayed fields also introduce their unchanged targets during
  layout preparation. This is a single-hop context expansion; the target's children
  are not automatically expanded. Clicking an object only filters already-prepared
  edges and does not alter the layout or viewport. Unchanged type references are
  eligible regardless of Show unchanged relationships; other relationship kinds
  retain that option. Initial and cleared selection still show no relationship lines.

### Confirmed decisions

| Decision | Answer | State |
| --- | --- | --- |
| Q1: Declaration scope | Fields/properties and their nested type expressions, showing which types each field is associated with in a class diagram. | Accepted |
| Q2: Unchanged type relationships | Show the selected field's type relationships even when unchanged; prepare target context before selection so objects and viewport stay in place. Other relationship kinds retain existing options. | Accepted |

The reusable `structure-field-types` preview includes an unchanged relationship
through a changed generic wrapper, a field switching between two Type targets, a
recursive field and unrelated unchanged call relationships.

### Field type relationship verification

- Structure-review tests pass: 78 passed and 11 opt-in tests ignored. The final Rust
  adapter rerun passes 37 tests with 10 opt-in tests ignored; rustfmt and all-target
  Clippy with `-D warnings` pass. Syntax and stdio tests verify nested type positions,
  omitted non-field queries, exact targets, deduplication, generic parameters and
  rejection of graph-external source before parsing it.
- A real rust-analyzer fixture verifies cross-file targets, imported names, two
  distinct same-name types, declared aliases, nested and function-pointer field types,
  tuple/enum/union fields, `Self`, test exclusions and graph-external omission. Changing
  a field target produces one removed and one added relationship; changing only its
  graph-external generic wrapper preserves the relationship. The existing real-server
  non-field type/read omission regression also passes, retaining calls, writes,
  function values and implementation relationships.
- The full frontend suite passes 42 files and 366 tests with two workers. An initial
  unrestricted run timed out in the unrelated GitHub authorization test while other
  checks were running; the complete rerun passes without changing that test.
  TypeScript, changed-source ESLint and the production frontend build pass. The
  existing Vite large-chunk warning remains. No installer is built for this follow-up.
- Independent Chrome completes 24 screenshot, accessibility and geometry captures at
  1440×1000, 900×800 and 520×800 in light/dark themes. Default and cleared states have
  no edges or connection markers; field selection shows unchanged, added/removed or
  recursive type edges from the precise field row. An unrelated field or function
  hides those edges. Target cards are present before selection; object positions,
  viewport transform and canvas identity remain unchanged. Long signatures, sidebar
  scrolling and the display menu remain contained, with no horizontal page overflow
  or browser exception. Viewport overrides are cleared afterward. The in-app preview
  request remains queued and no in-app interaction tool is available; in-app
  interaction verification remains pending.

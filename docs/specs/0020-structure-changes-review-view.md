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

The Review Lifecycle grants a Snapshot-scoped use of its existing head directory. It
verifies Review/Snapshot identity, Trust and the expected head while establishing the
protection. Refresh and close revoke active analysis, wait for its worker and language
server processes to stop, and only then mutate or remove head. Releasing the use does
not remove head; normal Review lifecycle remains its owner. A bare path lookup is not
sufficient, and cancelling only the frontend await is not sufficient.

Preparation and release must be owned by one Module:

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
| Q3: first language coverage | Existing Rust Adapter; explicitly show unsupported changed-file coverage | Accepted |

Q1 chooses a combined graph. Q2 changes the initial
presentation only, not the complete extraction inside the chosen file scope. Q3 limits
the first implementation to the existing Rust Adapter.

The first entry to this tab requests analysis lazily. A completed result is reused for
the same Snapshot; the page offers retry after failure and cancellation while running.
Selecting a node displays its declaration/content details rather than inventing source
navigation from its opaque ID. Layout is read-only, with pan, zoom, fit and context
expansion. These defaults implement the accepted scope without additional language
implementations, persistent graph storage or a new file-scope picker.

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
a duplicate edge; all original relationship changes remain in object details.
The graph contract and Rust extraction are unchanged by this presentation policy.

ELK arranges cards while member ports have fixed positions. React Flow connects
`sourceHandle` and `targetHandle` to those exact members. See the
[React Flow handle documentation](https://reactflow.dev/learn/customization/handles)
and [ELK port constraints](https://eclipse.dev/elk/reference/options/org-eclipse-elk-portConstraints.html).

### Reference display defaults

Following the user's local review feedback, imports and references outside the
repository are hidden by default. A Display options menu offers independent controls
for all objects, unchanged relationships, imports and dependencies outside the
repository. The first two controls do not override the dependency filter. Standard
library and third-party references require the dependency option; a repository symbol
outside the selected graph remains visible under the usual changed-reference rules.
Hidden references do not create floating target cards. Object details apply the same
import/dependency filters and identify when no relations match the current filters.

`code.type_use` and `code.reads` are no longer extracted by the Rust adapter. The earlier
type-use display checkbox is removed. Signatures and source code still include type
annotations and value reads, and structural relationships such as implementation and
membership remain available.

Both graphs carry `externalSymbols`, keyed by the unchanged opaque External identity.
The language analyzer supplies each name and `repository`/`dependency` scope. Cards
and details display those names rather than percent-encoded IDs such as `%2F`.
If a description is unavailable, show a generic resolved-target label and retain
unknown scope; never guess a dependency from a language's ID format. These are display
choices: complete graphs and graph differences remain intact. Import/dependency
reference toggles do not change the changed-object count.

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

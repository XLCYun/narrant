---
status: accepted
---

# Rust Language Analyzer

Implement Rust's `LanguageAnalyzer` using rust-analyzer, producing the existing
Code Graph for one prepared repository state and a caller-selected set of files.
The user selected rust-analyzer on 2026-09-11 after a local executable spike.
The design interview concluded on 2026-09-12 with Q1–Q8 accepted. This document
records the agreed scope and the implementation obligations.

## Accepted starting point

- Use rust-analyzer for Rust analysis.
- Reuse the existing [language contract](../../src-tauri/src/structure_review/code_graph/analyzer.rs)
  and [Code Graph contract](0018c-rust-interface.md). The caller supplies the repository
  root and repository-relative files for one fixed side of a Change Comparison.
- Extract all supported Code Objects in those files before graph comparison. Semantic
  resolution may read the wider project without adding every resolved object to the graph.
- Keep the existing common categories, native kind, language per node, single containment
  placement, outgoing references, and distinction between Node, External, and Unresolved.
- Preserve node and external-target identities across different absolute checkout paths.
  Object rename/move inference remains outside the accepted common comparison contract.
- Store language-normalized own code rather than an opaque fingerprint for body-only
  comparison. The user explicitly changed the earlier no-body-source constraint, then
  accepted normalization in Q5 on 2026-09-12.
- The first graph contains explicit source declarations. A declaration annotation such
  as `#[derive(Clone)]` stays with its type's declaration; generated methods are not
  automatically expanded into additional graph nodes.
- Invoke an existing external rust-analyzer binary from a Git-ignored local tool path.
  Automatic acquisition/downloading is future work.
- Obtain semantic information through the external server's LSP interface. Follow the
  accepted Q6 recommendation with a compatible, pinned local rust-analyzer syntax
  library for source structure; do not depend on viewSyntaxTree for production parsing.
- Keep repository revision selection/preparation and frontend presentation outside this
  analyzer. Do not mix repository snapshots with unrelated editor buffers.
- A selected-file read or extraction failure is an analysis error. Unavailable semantic
  analysis must not be represented as a proven absence of objects or relationships.

The common module now implements normalized own-code comparison. The concrete adapter
is implemented under `structure_review::rust_analyzer`; real-server verification is
recorded separately from the historical spike below.

## Test-code analysis scope

The common `AnalysisOptions.include_tests` switch defaults to false. The Rust adapter
omits ordinary unshadowed built-in `#[test]` declarations, and code whose `cfg` or
`cfg_attr` attributes prove that it can only exist with `test` enabled. This includes
inner `#![cfg(test)]` attributes and contained declarations. Conditions are evaluated
conservatively with other cfg flags unknown: `all(test, unix)` is test-only, while
`any(test, feature = "support")` and `cfg_attr(test, test)` do not establish that the
whole declaration is test-only. Always-disabled general cfg branches are retained.

Potentially shadowed short-name `test` attributes are confirmed through LSP definition
queries before graph assembly. A target must identify the `test` macro in sysroot source
with `rustc_builtin_macro`; a custom same-name procedural attribute, unavailable source
or unresolved definition stays included. The result for each definition is cached, so
several test functions reuse one source-identity check. Ordinary unshadowed tests and
explicit test-only cfg scopes do not need these additional attribute queries.

Potential imported-attribute shadowing is tracked per lexical scope; an import inside
another module or a function body does not suppress recognition of an outer test.
Attribute aliases with other names, framework macros, Cargo test targets and file/function
naming conventions do not provide additional classification. Explicit test-only cfg
still applies when an attribute macro's identity is unknown.

File-level inheritance follows rust-analyzer's parentModule responses, including
`#[path]` and parents outside the selected files. Parent responses and file decisions
are cached within the immutable analysis session. A shared file is excluded only
when every reported parent context is positively test-only. Cycles or missing
ownership evidence do not establish exclusion. Context reads do not enlarge the graph.

Local test subtrees are skipped during object extraction. Files with inherited test
scope contribute no graph nodes. Both omit body-definition and call-hierarchy queries for
excluded code; returned call ranges inside excluded code are not assigned to an outer
production object. Excluded targets are not replaced by synthetic unknown objects.
Excluded declarations and expressions are also removed from the parent's normalized
content, so editing tests alone cannot change retained business objects. Setting
include_tests to true restores the supported declarations and their relationships.
The language server may still load test source as part of workspace context; this
option does not promise to eliminate its project-loading or build-script work.

## Clarifications accepted during the first round

### Code content and generated declarations

Storing code supersedes the fingerprint recommendation. Code Object Content means the
content directly owned by a node; separately modeled children remain separate comparison
subjects. Q5 subsequently settled normalization rather than original-source preservation.

For example, `#[derive(Clone)] struct User;` contributes the explicit `User` Type node
with its derive annotation in the declaration. The generated `Clone::clone` method does
not become another graph node under the agreed initial output policy.

If a call is resolved to the concrete generated method `<User as Clone>::clone`, the
analyzer can retain a `code.calls` reference to that identified method as External,
because the target is absent from this graph's node table. External does not mean that
the method belongs to a different repository. If only the spelling `clone` is known
and a stable target identity cannot be established, the target remains Unresolved.

This is a target-identity condition, not a claim that a particular LSP response already
supplies a stable cross-checkout symbol ID. rust-analyzer's internal HIR models generated
derive methods with their implementing type and trait, but standard call hierarchy
items expose names, locations and optional data without guaranteeing stable identities
across repository revisions or server sessions. The adapter must obtain sufficient
evidence before assigning External.
[HIR representation](https://github.com/rust-lang/rust-analyzer/blob/master/crates/hir/src/lib.rs),
[LSP Call Hierarchy](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.17/language/callHierarchy.md).

### Project loading belongs to rust-analyzer

The adapter delegates project discovery, Cargo metadata loading, crate graph construction,
module ownership and `#[path]` semantics to rust-analyzer. These are not separate Rust
features that the user needs to elect to reimplement. The previous Cargo-versus-workspace
scope question mixed server capabilities with adapter implementation responsibilities.

Initialize supplies the prepared repository as workspace context. Use server discovery
by default rather than the spike's fixed crate name and fixed `linkedProjects` value.
The official discovery implementation checks the current/ancestor project manifests and
can look one directory level down for Cargo manifests when needed, which covers this
repository's `src-tauri/Cargo.toml` layout. It is not an unrestricted recursive search;
an explicit project path remains available when discovery cannot identify the intended
project. The production adapter's real-server test has now verified discovery in this
repository without specifying linkedProjects; the earlier spike used a fixed project.
[Initialization](https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/session.rs),
[Project discovery](https://github.com/rust-lang/rust-analyzer/blob/master/crates/project-model/src/lib.rs),
[linkedProjects configuration](https://rust-analyzer.github.io/book/configuration.html#rust-analyzer.linkedProjects).

The adapter still owns server lifecycle and document synchronization, selected-file
output filtering, conversion to CodeNode/contains/references, cross-checkout identity,
and error propagation. It must preserve distinctions when a source file participates
in several modules or crates instead of taking an arbitrary first match.

### Local server executable

For development, use `.tools/rust-analyzer` (`.tools/rust-analyzer.exe` on Windows)
under a Git-ignored `.tools/` directory in the application's development checkout.
Resolve that path before constructing the analyzer and pass the executable path into
the concrete adapter. Do not assume that `AnalysisInput.repository_root`, which may
point to any analyzed repository, contains the tool binary.

This slice requires an already supplied executable. A missing or unusable binary must
produce a clear analysis failure; there is no implicit download fallback. Binary
discovery/provisioning does not change the common AnalysisInput contract.

## Accepted code representation and integration

On 2026-09-12 the user accepted Q5's normalized-code recommendation and Q6's
recommendation, explicitly confirming that semantics should be obtained through LSP.

- Keep declaration signatures as normalized, readable declaration text.
- Store implementation content as normalized code directly owned by the node. Ordinary
  function bodies belong to Callable; do not insert a mandatory ExecutionRegion child.
- Exclude separately represented children's content from their parent's own comparison.
  Child containment and outgoing references retain their separate comparison rules.
- Preserve significant tokens and literal contents. Ignore pure formatting and ordinary
  comments using syntax-aware normalization, not textual whitespace/comment stripping.
- Extend common own-content comparison so a body-only edit reports Modified on its owner.
  No fingerprint, complete original-source copy, or common structured type AST is needed.
- Use a pinned, compiler-compatible local rust-analyzer syntax crate for declarations,
  source structure and normalization. Use the external server for semantic resolution
  over LSP; the syntax library does not replace project loading or name/type resolution.

This responsibility split is internal to the Rust adapter. The common
LanguageAnalyzer input remains the prepared repository root and selected relative files.
Library version selection, protocol framing, process cleanup, and position conversion
are implementation responsibilities, not further user capability choices.

### Consequences for the existing common contract

The smallest data extension is an optional normalized `CodeNodeData.code` alongside
`signature`. A function's body and a const/static initializer belong to their Callable
and Value respectively. `None` means the object has no corresponding implementation
content, not that extraction failed; an empty function still has an empty body to compare.
Containers continue to organize declarations rather than embedding their descendants'
source in this field. The virtual root, node table, contains and references need no new
shape for this extension.

The own-data comparison must include code. Consequently, replacing a call can produce
both an own-content modification and reference removal/addition. These dimensions are
independent, not mutually exclusive. Rename `NodeDiff.declaration` to `NodeDiff.content`
to reflect declaration and body changes together. This retains the existing own-data status rather than adding
field-level diffs or a fourth diff dimension.

On 2026-09-13 the user requested removing standalone closure objects and confirmed
that calls from inside closures must still be resolved. Closures are now expressions
in their enclosing object's normalized code, with no Callable or ExecutionRegion
node and no child-identity placeholder. Changing closure parameters, capture syntax,
body or use order modifies that enclosing object's own content. References inside
closures are still resolved through LSP and belong to the nearest modeled declaration.
This records call sites in the owner's code; it does not claim that constructing a
closure executes its body immediately.

Directly invoking a closure, or invoking a binding known to initialize one, does not
create a synthetic target or a self-call to the containing function. Unknown dynamic
callables, including function-pointer parameters, retain Unresolved references.

Nested named items such as `fn helper` and `const N` are independently
declared children: reordering those declarations alone must not fabricate a parent-code
modification. A reference to `helper()` or `N` in the parent's executing expression still
belongs to the parent. These projection details implement the accepted own-content
boundary, including named items declared inside a closure.

### Relationship extraction scope

The Rust adapter no longer emits `code.type_use` or `code.reads`. It skips definition
queries for type annotations, bounds, record literal/pattern type names and read-only
fields. Types and data declarations remain Code Objects, and their signatures and own
code are still compared. Reading a different value can change an owner's code without
producing a separate read edge.

Calls, imports, writes, macro use and structural relationships (`code.applies_to`,
`code.implements`, `code.member_of`) remain. Compound assignments emit a write edge
without a read edge. General path expressions still need semantic resolution to
distinguish function values from data/type values: function-value references remain
`code.references`, while confirmed data/type targets are omitted rather than relabeled.
Unresolved general references retain their existing uncertainty.

Skipping those source queries reduces LSP round trips and target-description work;
rust-analyzer still loads the workspace and performs the semantic analysis needed for
the retained queries. No fixed end-to-end speedup is guaranteed.

### Analysis context and failure handling

rust-analyzer normally analyzes an effective Cargo/cfg/platform configuration, rather
than enumerating every platform and feature combination. Current upstream defaults use
Cargo default features, no extra requested features, the configured target platform or
host platform, and enabled build scripts/procedural macros. Local crates also receive
test and analyzer cfg settings. Cargo `allTargets` refers to lib/bin/test target kinds,
not every target platform. The production adapter must record/test its chosen server
configuration; the spike's explicitly disabled build scripts/macros are not an accepted
production default.
[Configuration](https://rust-analyzer.github.io/book/configuration.html),
[Crate configuration](https://github.com/rust-lang/rust-analyzer/blob/master/crates/project-model/src/workspace.rs).

The local syntax library can still extract declarations under inactive cfg branches.
rust-analyzer's name collector skips cfg-disabled items, so reliable standard-LSP
resolution inside every such branch is not guaranteed. Q7 accepted a single effective
semantic configuration: retain all explicit declarations, including inactive branches,
and record unresolved targets explicitly. Complete
multi-configuration analysis remains future work.
[cfg handling](https://github.com/rust-lang/rust-analyzer/blob/master/crates/hir-def/src/nameres/collector.rs).

The existing common failure contract distinguishes an observed reference with an unknown
target from a failed extraction. The adapter must therefore distinguish server/project
readiness, successful query execution, and target resolution. EOF, timeouts, missing
required capabilities, or JSON-RPC errors do not become empty relationship sets.
Likewise, failed project/dependency/build/proc-macro loading must be surfaced as analysis
failure when it prevents the promised semantic extraction; no implicit partial-graph
result is introduced. A successfully observed reference whose target remains unknown
can use Unresolved without claiming that the whole analysis failed.

The server-status extension exposes `quiescent` and `health`, but significant loading
failures can be `warning`, not only `error`. It is not a certificate that every symbol
resolves. The spike rejected only `error`; production verification must cover degraded
loading and request failures rather than copying that optimistic check.
[Server Status](https://rust-analyzer.github.io/book/contributing/lsp-extensions.html#server-status),
[Readiness and health](https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/reload.rs).

## Evidence from the local spike

The spike ran against repository commit `e058a10c7c7b5c4bf2c3506727c89ca0245d5164`
with `rust-analyzer 1.97.1 (8bab26f4 2026-07-14)` on macOS arm64.
Its untracked local source and protocol evidence are in `.spikes/rust-code-graph/`
in the original checkout. This specification retains the findings needed for decisions;
the temporary program is not the production implementation.

Seven selected files produced 104 nodes and 198 outgoing references, validated by the
production `CodeGraph::new`. The graph had one virtual-root entry, the explicitly declared
`code_graph` module. The experiment also used the production `CodeGraph::diff`.

Observed behavior:

- Repeated analysis and comment/formatting edits, including Chinese and emoji before a
  declaration on the same line, produced an empty graph difference.
- A method visibility change modified only that method's own declaration.
- Replacing a call with a self-call changed only the caller's references.
- Adding/removing a function changed that function and its containing module's child set.
- `validate`, from an unselected file in the same repository, was an External target.
  Selecting its file in a second run made the call target a graph-local Node.
- `Self` required language-specific interpretation to resolve the actual implemented type,
  rather than treating an impl-header position as a different external object.
- Replacing `==` with `!=` inside a function produced an empty graph difference. This
  reproduces the missing own-content comparison capability; it is not a successful detection.

The spike used document symbols, definition lookup, outgoing call hierarchy and the
`rust-analyzer/viewSyntaxTree` extension. It did not independently model four closures
or derive-generated objects. Two resolved generated `clone` calls were omitted from the
graph; five generic-parameter references were deliberately outside its high-level object set.
This observed omission does not establish acceptable production behavior.

## Technical facts relevant to the decisions

Standard LSP and rust-analyzer's internal Rust APIs have different stability expectations.
The official architecture document describes standard LSP as stable and the `ide` API
as unstable. `viewSyntaxTree` is a debugging extension whose response is a JSON string;
its internal `kind/start/end/children` schema is not given the same stability guarantee.
[Stability guarantees](https://rust-analyzer.github.io/book/contributing/architecture.html#stability-guarantees),
[View Syntax Tree](https://rust-analyzer.github.io/book/contributing/lsp-extensions.html#view-syntax-tree).

The syntax crate is a standalone, single-file syntax interface without semantic information.
Published `ra_ap_*` crates use coordinated versions and exact internal dependencies;
using the syntax crate does not establish a long-term stable Rust API contract.
There is no documented requirement found that a local syntax crate and an external LSP
server have identical versions when they exchange only source text and LSP positions.
That separation is an architectural inference, not a guarantee of compatibility for all
Rust syntax or editions. Version combinations need explicit validation.
[Syntax architecture](https://rust-analyzer.github.io/book/contributing/architecture.html#cratessyntax),
[Official publishing workflow](https://github.com/rust-lang/rust-analyzer/blob/master/.github/workflows/autopublish.yaml).

At the time of research, `ra_ap_syntax 0.0.351` declares Rust 1.98 while the local
toolchain is 1.97.1. This is a concrete version-selection constraint, not a reason to
silently upgrade the repository toolchain or assume that the latest crate will build.
[Versioned manifest](https://docs.rs/crate/ra_ap_syntax/0.0.351/source/Cargo.toml).

An in-process integration also has an official workspace-loading entry point in
`load-cargo`. It loads the database and VFS and can run build scripts and start a
proc-macro process; embedding the library does not imply that no child processes run.
Build time, binary size and memory differences have not been measured in this repository.
[Workspace loading](https://github.com/rust-lang/rust-analyzer/blob/master/crates/load-cargo/src/lib.rs).

Cargo metadata supplies actual packages, targets, crate roots and editions. The
`parentModule` query can identify parent declarations but can return multiple parents,
including when the same file is included under different module names. These sources
can replace the spike's hard-coded crate name and ordinary-path assumptions, but they
do not by themselves define a unique cross-context node identity.
[Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html),
[Parent-module implementation and tests](https://github.com/rust-lang/rust-analyzer/blob/master/crates/ide/src/parent_module.rs).

## Design tree and current frontier

| Decision | Current outcome | Status |
| --- | --- | --- |
| Q1: own-content comparison | Store code rather than a fingerprint; normalized form accepted in Q5. | Accepted |
| Q2: generated objects | Keep explicit declarations and their annotations; do not expand generated methods into graph nodes. Explain and validate generated-target identities separately. | Node policy accepted |
| Q3: project scope | Delegate project loading and Rust module semantics to rust-analyzer; do not reimplement its native project support. | Responsibility clarified |
| Q4: integration/provisioning | Invoke a binary supplied in a Git-ignored tool path; downloader deferred. Syntax acquisition accepted in Q6. | Accepted |

The second round settled the representation choices on 2026-09-12:

| Decision | Question | Recommendation | Status |
| --- | --- | --- | --- |
| Q5: code form | Preserve complete original source, or store language-normalized own code? | Store normalized code, consistent with normalized signatures and format-insensitive comparison. | Accepted |
| Q6: syntax acquisition | Use a local syntax library with LSP, or rely on the binary's syntax-tree extension? | LSP for semantics and a compatible, pinned local syntax library for source structure. | Accepted |

The final round accepted both output-boundary decisions on 2026-09-12:

| Decision | Question | Recommendation | Status |
| --- | --- | --- | --- |
| Q7: conditional compilation | Must initial semantic analysis combine multiple platforms/features, or can it use one effective configuration while retaining every explicit declaration? | Retain all explicit declarations; resolve references under the same chosen settings on both sides; use Unresolved when a syntactically observed reference cannot be resolved. Defer multi-configuration merging. | Accepted |
| Q8: item-position macro invocations | Where does a top-level `declare_items!(A)` invocation live when generated objects are not expanded and Container has no implementation body? | Represent the explicit item-position invocation as Extension, with normalized invocation code. Expression macros remain in their existing owner's code. | Accepted |

Q8 prevents `declare_items!(A)` becoming `declare_items!(B)` from disappearing from the
graph. Macro definitions can already use Extension. The accepted mapping adds a subject
for the explicit invocation itself, not nodes for all generated objects.

The later closure policy above supersedes the original standalone-Callable mapping.
Source/project identity must be derived without the
spike's fixed crate name or absolute checkout paths. Cases with several crate/module
contexts must not select an arbitrary first match; conservative identity matching does
not promise inference across anonymous-object reorderings or impl-block splits/merges.
Protocol/version selection and the detailed extraction algorithm remain implementation
work supported by fixtures, not further user capability-selection questions.

## Verification obligations for the implementation

Resolved External references also receive readable names and explicit repository
scope. The Rust adapter derives names from actual declarations, including enclosing
types for methods, and compares actual source paths against the analysis repository.
Standard-library, registry and Git dependency source outside that root is marked
`Dependency`; unselected repository source remains `Repository`. Only referenced
targets receive descriptions. The adapter preserves encoded symbol identities, and
the frontend neither displays nor reverse-engineers those IDs as names.

Use the current repository and small prepared before/after fixtures with the real server,
in addition to focused tests of the common own-content comparison and source projection.
The actual pinned server/library combination must be exercised. At minimum verify:

- Repeated analysis and relocation to a different absolute checkout root preserve IDs and
  yield no differences; formatting and comments do not create modifications.
- Function-body and Value-initializer literal/operator edits modify their owning nodes.
  Call replacements produce both code changes and the appropriate reference delta.
- Editing a separately modeled child method, nested function or constant does not
  recursively modify ancestors. Editing an inline closure modifies its enclosing
  object's code; its calls remain visible without closure or ExecutionRegion nodes.
- Literal contents, token boundaries and Unicode/UTF-16 positions survive normalization
  and semantic lookup. Requested-file read/parse failures produce analysis errors.
- Selecting a resolved target's file changes its representation from External to Node
  without inventing a new semantic identity. Generated targets are never silently dropped
  solely because they do not have explicit graph nodes.
- Explicit module declarations, impl/member ownership, macros, cfg branches, and repeated
  source contexts follow the accepted output decisions. Inactive declarations remain
  present with unresolved targets where necessary;
  item-position macro invocations are Extension nodes carrying their invocation code.
- Server absence, failed readiness, transport/query failures, timeouts and cleanup are
  exercised separately from successful queries with unresolved targets. Analyses do not
  modify source files or mix source states across the two graphs.

## Implementation placement

The concrete adapter lives in `src-tauri/src/structure_review/rust_analyzer/`, beside
the language-neutral `code_graph` module. `RustLanguageAnalyzer::new(executable)`
accepts the already provisioned server; its LanguageAnalyzer implementation owns
source validation, a fresh server session per snapshot, syntax extraction and semantic
resolution. Syntax extraction and graph resolution remain private to this Adapter.
The syntax crate is pinned to `ra_ap_syntax =0.0.349` (MSRV 1.95), compatible with the
current Rust 1.97.1 toolchain; the newer 0.0.350/351 require 1.98.

### Shared LSP Module and Rust Adapter

The dependency direction is `RustLanguageAnalyzer → RustLsp → LspClient`.
`src-tauri/src/lsp/` exposes a language-independent synchronous stdio session.
Its Interface accepts a configured process `Command`, workspace path and `LspOptions`;
document opening takes an explicit language identifier and version. It owns the
initialize/initialized handshake, request matching, fixed request deadlines, standard
reverse-request handling, bounded Content-Length framing, and process-tree cleanup.
Its private `transport.rs` hides the process and I/O workers; `protocol.rs` normalizes
standard positions, definition locations and outgoing-call responses. Call hierarchy
queries preserve the server's complete prepared item, including opaque `data`.

`LspClient` provides standard definition and outgoing-call queries, plus request/notify
methods for extensions. Latest notification parameters are retained by method name;
an Adapter can wait for its own predicate without adding language rules to the shared
Module. This is a snapshot-analysis session, not a complete editor client or event log.
The common Module does not require a particular language, graph contract, provider or
project-readiness extension.

`structure_review/rust_analyzer/lsp.rs` defines the private `RustLsp` Adapter by
composition. It supplies rust-analyzer initialization options, opens Rust documents,
requires the definition/call-hierarchy/parent-module capabilities and UTF-16 positions,
and interprets `experimental/serverStatus`. Semantic queries require a quiescent server
with health `ok`; degradation during a query is an analysis failure. The Rust-only
`experimental/parentModule` request and conversion into the analyzer's error type stay
here. `references.rs` calls named semantic methods and maps their typed results into
graph relationships without assembling JSON-RPC requests or decoding protocol variants.

Tests cross each Interface through a local stdio server. Shared-session tests use
non-Rust documents and a server without Rust capabilities, and cover interleaved reverse
requests/notifications, opaque call data, malformed frames, query failures, timeouts and
cleanup. Rust Adapter tests separately cover its configuration, capability checks,
readiness order and degraded status. The existing real rust-analyzer graph tests cover
the complete composition.

### Concrete extraction conventions

- A selected Cargo crate entry has a `Container` with native kind `crate` when
  `experimental/parentModule` identifies it by returning a `Cargo.toml` location.
  The Rust LSP Adapter distinguishes this result from a Rust module declaration;
  the graph builder puts the entry's top-level declarations inside that root.
  Library, binary and integration-test entry files therefore retain separate roots.
  Ordinary files and successful empty parent responses do not imply a crate root.
- Root IDs reuse the existing `<source-identity>::crate` convention, so local and
  external references keep the same identity across selection changes and checkouts.
  The root's readable name is its repository-relative entry path, with native kind
  `crate`; it is not a guessed Cargo target name. Its signature and own code are
  absent. Member changes are compared through `contains`, without marking the root's
  own content modified. Only selected roots are materialized, including empty roots;
  unrelated project targets are not added. The adapter does not run a second Cargo
  discovery process or interpret directory names as module ownership. Non-Cargo
  roots remain under the existing placement rules unless LSP supplies this evidence.
  This follows rust-analyzer's [parent-module handler](https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/handlers/request.rs)
  and was verified with rust-analyzer 1.97.1 through actual LSP sessions.
- Node identities combine a repository-relative source identity with lexical declaration
  ownership and names. They never use checkout prefixes or line/column positions.
  Repeated anonymous/same-header declarations use local occurrence disambiguators;
  reordering or inserting ambiguous siblings can change correspondence. The common
  module does not infer anonymous moves or impl splits/merges.
- Source objects are stored once. Cross-file parent module links come from
  `experimental/parentModule`, including `#[path]`. When a shared source has several
  module parents, its declarations stay at the graph root with all `code.member_of`
  relationships, preserving the single-placement invariant without picking one parent.
  This rule also applies when some of those parents are outside the selected files.
- Rust `use` and `extern crate` declarations and foreign declaration blocks use Extension
  nodes. This keeps top-level imports comparable even when their enclosing source module
  declaration is outside the selected files. Imports carry `code.imports` relationships.
- Ordinary expression macro invocations remain in their owners; explicit macro-use
  relationships use `rust.macro_use`. The syntax library does not parse arbitrary macro
  token trees as ordinary Rust expressions. LSP outgoing call hierarchy supplements
  resolvable calls from expansion, with ownership derived from original call ranges.
- Missing generated-target identity is retained as Unresolved. A location on a derive
  annotation or a type declaration is not evidence that the generated method is that
  Type node.
- Taking a function as a value uses `code.references`; only a call expression or LSP
  outgoing-call observation uses `code.calls`. Data writes use `code.writes`; data reads
  and general type uses are omitted from graph relationships.

### Calling the adapter

Both repository directories must already represent the desired immutable states.
The file lists contain only files that exist on their respective side; deleted or
added files need not appear on both sides. The same analyzer can be reused because
each call starts a separate LSP session.

```rust
use crate::structure_review::{
    code_graph::{AnalysisInput, LanguageAnalyzer},
    rust_analyzer::RustLanguageAnalyzer,
};

let analyzer = RustLanguageAnalyzer::new("/path/to/narrant/.tools/rust-analyzer");
let before = analyzer.analyze(&AnalysisInput {
    repository_root: "/path/to/before".into(),
    files: vec!["src/lib.rs".into(), "src/store.rs".into()],
})?;
let after = analyzer.analyze(&AnalysisInput {
    repository_root: "/path/to/after".into(),
    files: vec!["src/lib.rs".into(), "src/store.rs".into()],
})?;
let diff = before.diff(&after)?;
```

The synchronous call belongs on an application worker thread. Server project loading
retains default Cargo build-script and procedural-macro behavior and therefore belongs
under the application's existing Workspace Trust boundary. This adapter does not add
an executable downloader, frontend command, or snapshot-preparation operation.

## Relationship scope verification on 2026-09-13

- The affected Structure Review suite passed: 60 tests, with 6 real-server tests
  excluded from the ordinary run. A stdio protocol test verifies that type mentions,
  a record constructor and a read-only field make no definition requests; the retained
  call is the only definition request in that fixture. Signatures and code remain intact.
- A dedicated real rust-analyzer comparison passed, covering local and unselected-file
  type/data targets, field reads, compound writes, closure calls, function values and
  implementation relationships. Neither removed semantic appears in the result, nor
  are confirmed type/data reads relabeled as general references. Changing a read
  expression still modifies only the owning method's code, with no read-edge delta.
- The prepared-repository real-server regression passed separately. These fixtures
  verify behavior and omitted request sites; they do not measure a repository-wide
  speedup. Rustfmt and all-target Clippy with warnings denied passed.

## Closure extraction verification on 2026-09-13

- The affected Structure Review suite passed: 59 tests, with 5 real-server tests
  excluded from the ordinary run. Tests cover closure body/signature changes in
  functions and const/static values, nested closures, formatting, and named items
  declared inside closures.
- A dedicated real rust-analyzer comparison verified no closure or ExecutionRegion
  nodes, preserved outgoing calls to an unselected repository file, and a single
  modified enclosing function plus reference removal/addition when a closure's call
  target changes. Direct, bound, cast and const closures do not create fake unresolved
  targets or enclosing-function self-calls; dynamic function-pointer parameters still
  retain unresolved calls.
- The prepared-repository integration regression passed. A real analysis of seven
  selected files in this repository produced 130 nodes and 261 relationships with no
  closure nodes. This count describes that source scope, not the complete PR.
- Rustfmt and all-target Clippy with warnings denied passed. The frontend transport
  and rendering contract are unchanged by this extraction policy.

## Implementation verification on 2026-09-12

Verified on macOS arm64 with Rust and rust-analyzer 1.97.1 (8bab26f4 2026-07-14),
using the pinned `ra_ap_syntax 0.0.349`. The tool executable and local graph evidence
are Git-ignored under `.tools/` in the `codex/rust-language-analyzer` worktree.

- Focused shared `lsp` and `structure_review` tests: 52 passed. The two real-server
  tests are ignored in normal Cargo runs and executed explicitly against the supplied
  binary.
- Clippy for all targets with warnings denied passed.
- Both real-server tests passed. Seven selected files in the current repository
  produced 123 nodes and 246 outgoing relationships. Prepared repositories verified
  stable identities across absolute checkout roots, a body-only modification on `run`,
  closure call ownership, imports and data writes, generated/inactive unresolved targets,
  external-versus-selected target identity, and multiple module parents.
- After the LSP separation, the full Rust suite passed: 542 passed, zero failures,
  and the two explicitly run real-server tests ignored in the normal suite.
- The initial implementation's earlier full Rust run had 533 passed, one failure,
  and two ignored. The failure was the unchanged
  `core_git::tests::cancelled_workspace_creation_removes_its_partial_worktree` test:
  cancellation encountered an initializing Git worktree whose `.git` file was not yet
  ready for cleanup. An exact focused retry passed, and the current full-suite run
  also passed. This historical failure does not establish a proven diagnosis of a
  pre-existing race.

Normal checks use the repository's shared Cargo lock and cache wrapper:

```sh
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --lib --quiet -- lsp:: structure_review::
scripts/cargo-agent.sh clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

For explicit real-server verification, compile through that wrapper, then run the
resulting test executable with `real_server_ --ignored --nocapture --test-threads=1`.
Set `NARRANT_RUST_ANALYZER` to the supplied executable; optionally set
`NARRANT_CODE_GRAPH_OUTPUT` to a local evidence directory. During agent verification,
the server's Cargo subprocesses also use a local proxy through `scripts/cargo-agent.sh`.
Run the test executable outside an already-held Cargo wrapper lock to avoid a nested
lock while rust-analyzer loads the project. The local `.tools/run-real-tests.py` runner
records this setup; it is temporary verification tooling, not part of the adapter API.

Crate-root verification uses the public analyzer interface. Two stdio tests cover
separate library/test roots, custom paths, empty parent responses and stable graph
identities across repository directories. Real rust-analyzer 1.97.1 sessions verify
library/test containment, normal child modules, cross-crate calls, member-only diffs,
selection scope, custom Cargo entries and an empty crate. The existing multi-parent
and cross-checkout real-server regression also passes. The full Rust suite passes
561 tests (8 ignored), with the three relevant real-server tests run explicitly;
`check --lib`, rustfmt and all-target Clippy with warnings denied pass.


### Test-code scope verification

- The public analysis contract defaults to excluding tests and passes identical
  options to both comparison sides. Syntax and stdio tests cover built-in tests,
  outer/inner cfg, cfg_attr, unknown/shared conditions, shadowed attributes, nested
  declarations, cross-file parents and shared production files. They verify omitted
  definition/call-hierarchy queries, discarded test call ranges and test-only edits
  producing no changes in retained parent content or containment.
- The full Rust run completed with 571 passing tests, 9 ignored tests and one failure
  in the unchanged Core Git process-output-limit test: output-pipe cleanup exceeded
  its timeout and returned TerminationFailed. That exact test passed when rerun alone.
  All structure-review tests passed in the full run. After the lexical-scope refinement,
  the focused structure-review suite passes 74 tests with 9 real-server tests ignored.
- `check --lib`, rustfmt and all-target Clippy with warnings denied pass. Explicit
  real rust-analyzer verification passes for test exclusions/restoration, inherited
  test scope, shared files and an empty tests-only diff. The existing prepared-repository
  regression also passes with tests included. The current repository regression also
  passes, producing 133 nodes and 112 references from the selected files.


### PR31 wildcard-import regression

The original policy treated `use httpmock::prelude::*` as possible attribute shadowing
and kept all four built-in test functions in PR31's `tests/github_discovery.rs`. A
reduction to one empty test function reproduces it; removing only the wildcard import
or using explicit cfg(test) restores exclusion. This failure also reproduces through
the public analyzer with the original PR source and real rust-analyzer.

The adapter now confirms ambiguous test attributes before graph nodes and relationship
queries are assembled, including for context documents. Confirmed ranges are merged
with syntax-based exclusions and declarations are rebuilt with the same normalization
rules, so no removed child identity or parent content remains. Custom attributes and
unknown definitions retain the conservative behavior. This does not infer that an
unannotated helper is test-only merely because it is under tests/ or called by tests.

- The real-server public-interface regression fails before the fix and passes after
  it using the original PR31 source. All four built-in tests are excluded by default
  and restored when requested; the unannotated helper and a same-name custom
  procedural attribute remain included. The existing real-server cross-file cfg
  regression also passes.
- A stdio regression verifies attribute identity queries, omitted test-body queries,
  unresolved attributes being retained and no extra attribute queries when tests are
  included. A syntax regression verifies that semantic exclusions merge with nested
  cfg exclusions without dangling containment or changes to the retained parent.
- The affected structure-review suite passes 76 tests with 10 real-server tests
  ignored in the ordinary run. Rustfmt and all-target Clippy with warnings denied
  pass. The public graph contract and frontend are unchanged by this correction.

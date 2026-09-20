# TypeScript Code Graph implementation

Spec: [0021](../specs/0021-typescript-language-analyzer.md). Start: 2026-09-15.
Research evidence is in docs/research/typescript-lsp-validation.md and the integration checklist.
Execute through all tasks; commits remain on codex/typescript-code-graph for review.

## Global Constraints

- Oxc owns syntax/own content; TLS 6.0.0 + tsserver 6.0.3 owns semantics through shared LSP.
- TS/TSX/JS/JSX/MTS/CTS/MJS/CJS/declarations are one semantic group. Preserve native JSDoc.
- Default context is repository + fixed standard/runtime declarations, excluding third-party business code/types.
- Direct const-bound arrows are Callable; anonymous inline callbacks stay in owner code. Intrinsic JSX has no nodes/edges.
- Preserve six graph categories, source-declaration identity, own-content boundaries, and Node/External/Unresolved.
- Before/after use their own snapshots/configs and the same fixed engine/rules/options. Any hard language failure fails the whole comparison.
- Each side's languages read the same Git workspace; no per-language source/config/declaration copies. Temporary helper/control files are allowed.
- Chinese comments for functions/types. Private Rust tests in module tests/ directories, never inline.
- Every Cargo command through scripts/cargo-agent.sh; separate worktree target, no disabled incremental, no manual quality-gate.
- Use /Users/lizhanfa/.nvm/versions/node/v24.18.0/bin in PATH for commands; the login shell currently chooses unsupported Node 26.
- During each task run focused tests/checks only; the controller schedules one final full Rust suite.
- Implementers do not spawn agents. Only the controller dispatches reviews; one implementation agent at a time.

## Task 1: Oxc document model and source objects

Implement the TS/JS syntax foundation under src-tauri/src/structure_review/typescript_analyzer/.
Read spec 0021 sections Language and objects, Content and identities and Relationships.
Add exactly pinned compatible Oxc crates (research baseline 0.150.0; verify APIs/MSRV) to Cargo.toml/lock.
Expose the module in structure_review/mod.rs and a small internal Result/failure seam.
Do not implement the LSP/project/application layers in this task.

Suggested files: mod.rs, syntax.rs plus focused syntax/ submodules, identity.rs, text.rs,
test_code.rs and tests/. Split by responsibility instead of one oversized AST visitor.
Use Oxc typed AST/visitor and code generation where useful; no regex-based structural parsing.
AST arena lifetime should not leak into the rest of the adapter: Document can own source,
stable declarations/occurrences/spans after a scoped parse/visit. Avoid a self-referential struct.

Provide an internal Document::parse(repository: &Path, path: PathBuf, source: String,
options: AnalysisOptions) -> Result<Document>. It owns path/URI/source/language, declarations
and reference occurrences. Each declaration supplies CodeNode, lexical parent, full span,
all name/overload selection spans, and any needed callable/body boundaries. Provide owner
lookup and exact declared-at lookup. This is the stable seam consumed by Task 3.
Document spans can use oxc_span::Span or a compact equivalent; provide checked UTF-16
conversion with crate::lsp::Position. Occurrences should carry semantic intent, position,
observed text and flags needed to reject dynamic/type-only/import-alias false calls.

Implement all objects/arrow/test/JSX/overload/merged-declaration rules in spec. Capture
calls/new, member/callee names, function-value references, writes, imports/reexports,
heritage and field/property type uses for later semantic resolution. Keep inline callback
occurrences assigned to their nearest modeled owner. Intrinsic tags are excluded as
relationships while expressions inside JSX still get traversed.

Normalization must not mix child content into parent own content or erase meaningful
JS values. Preserve full source separately. Use readable signatures and normalized code;
source ranges and generated temporary markers must never destabilize identity.

Focused tests through Document/CodeGraph/diff must cover the file family, direct const
arrows vs callbacks/wrappers, class members and nested named functions, overloads,
cross-file declarations, no duplicate Value, JSX expressions/intrinsic filtering, tests
toggle, stable IDs across roots/selection, formatting/comments vs ASI/templates/regex/JSX
text, and Unicode/CRLF conversion. Invalid source/paths fail, not success empty extraction.
Run cargo-agent check --lib and syntax-focused tests, not the full suite. Report the
precise internal interfaces and any remaining Task 3 semantic requirements. Commit only
task-owned files and update no research/spec prose unless a factual correction is needed.

## Task 2: Shared Git workspace and fixed LSP sessions

Implement project.rs/project/ modules, tools.rs, lsp.rs and trusted runtime helpers under
typescript_analyzer/runtime/, plus their tests. Consume Task 1 source/path helpers only
if useful; do not alter its graph extraction behavior or implement application/UI.
Read spec Project input and defaults, Tools and lifecycle, and research source constraints.

Define TypeScriptToolchain with explicit Node path and runtime root. Development defaults:
NARRANT_TYPESCRIPT_NODE override or repository tool directory .tools/node (node.exe on
Windows); NARRANT_TYPESCRIPT_RUNTIME override or .tools/typescript-analyzer. Runtime
contains typescript/, typescript-language-server/, @types/node/ and undici-types/.
Validate fixed TLS/TS versions and Node >=22.22.2, paths and runtime dependency closure.
Validate lazily on nonempty analysis; no download from the runtime analyzer.
Add an explicit scripts/setup-typescript-analyzer.mjs and package script to obtain fixed
packages with integrity verification/no package install scripts and to prepare the Node
tool entry from the running supported Node. Preserve existing app dependency versions.
Node types initial profile is @types/node 22.20.1 + undici-types 6.21.0.

Read the provided repository side directly. Preserve first-party layout, JSONC config,
extends/references and manifest module metadata. Override configuration and first-party
package lookup paths only in memory through the fixed tsserver file host; never create
source/config/declaration copies or physical package links. Read fixed runtime declarations
at their installed paths and exclude business sources/declarations from service inputs.
Do not force noResolve or flatten configurations. Configuration helper may use the fixed
TS library to parse/resolve config (no checker/graph extraction); embed trusted helper
source so packaged app does not depend on current repository source paths. Runtime helper
errors/timeout/cancel and malformed results must fail clearly and reclaim processes.

Support tsconfig/jsconfig and unconfigured selected TS/JS. Document deterministic inferred
profiles: configuration semantics first; for unconfigured Node package/context use NodeNext
with ES2022 and fixed Node declarations; otherwise Preserve/Bundler ES2022 with DOM and JSX
preserve. Explicit project lib/noLib remains authoritative. Treat missing business config
extends/types as an analysis note if reliable project context can still be established;
do not execute config JS/plugins or borrow original dependency sources.

Expose a PreparedProject owning only its helper/control TempDir, canonical workspace paths, project registrations,
allowed roots and per-file project candidates, notes and source snapshots. Validate all
selected input paths and context paths. Unexpected external source membership is an error.
Project info must be checked against actual requested project names (not registration aliases).

Build TypeScriptLsp on crate::lsp::LspClient. Fixed tsserver.path, ATA off, plugins empty,
syntax server never. Register controlled configs before didOpen; raw tsserverRequest uses
executionTarget:0. Supply definition and outgoing/incoming query APIs with explicit project
where ambiguous; normalize raw 1-based line/offset replies into checked location evidence.
Do not hide malformed responses or RPC errors as empty targets. Collect bounded per-project/
file analysis notes using explicit queries rather than last-notification storage. Ensure
shutdown/exit and cancellation reclaim the full process group before releasing temp files.
Modify shared LSP only for demonstrated protocol/lifecycle gaps and regress Rust consumers.

Focus tests on unchanged Git workspace files, actual service paths, helper cleanup without
workspace deletion, exclusion even with installed third-party types, standard Node closure,
JSONC/extends and paths, project references with absent dist, workspace package imports,
config-less JS, selected-file association, URI mapping/escape rejection, real server normal
shutdown and missing-tool/cancel/errors. Fixed-service tests can be ignored by ordinary CI
when tools are absent, but must be run explicitly here with the prepared fixed tools.
Do not silently skip a mandatory real-service verification when tools are available.
Run focused tests only, commit task-owned files, and report the exact Task 3 interface.

## Task 3: Semantic graph adapter and analysis reports

Implement the public TypeScriptLanguageAnalyzer using Tasks 1/2, references.rs/reference/
modules and focused real integration tests. Keep analyze(&AnalysisInput) -> CodeGraph to
implement LanguageAnalyzer. Also expose analyze_with_report returning graph + a Vec of
structured analysis notes (code, message, optional repository-relative file/project).
Define the language-neutral run report/notes in structure_review/analysis.rs if needed;
notes do not enter CodeNodeData or diff. Cloneable analyzer supports with_cancellation.

Parse selected files for output membership, other allowed repository sources lazily for
definition identity. TS and JS targets resolve using one shared membership index. Validate
LSP locations against actual declaration spans/kinds/alias chains. Selected => Node;
unselected repository/allowed runtime => External with stable description; unknown or
conflicting runtime targets => Unresolved. Never turn a local ImportSpecifier or ambient
call signature into a definite call. Follow the spec relationships including JSX reclassification,
deduplication, nested callback owner, same-owner overload mapping and merged fragments.

Run per-effective-project queries and aggregate conflicts conservatively. Keep reliable
facts when business dependencies are missing; emit bounded per-side-ready notes about
missing contexts. A failed service is an error, never automatic syntax-only success.
Validate sources did not change within the run and the final graph passes CodeGraph::new.
Validate selected paths before tests/empty short-circuit; excluded tests need no server.

Real tests must cover cross-file TS↔JS calls, aliases/barrel, Promise and Node wrappers,
Node ESM/CJS, JSDoc native inference, JSX and non-graph callbacks, properties/heritage/writes,
imports/reexports, overload/merged declarations, dynamic/ambiguous/missing targets,
references/workspace package missing dist, tests toggle, stable external identities and
normal/cancel/tool-failure cleanup. Exercise Graph diff for child/parent and format-only
changes. Run affected test suites, commit, report validation and remaining application seam.

## Task 4: Application composition and TypeScript presentation

Wire TS analyzer into Tauri bootstrap and StructureReview, retaining the existing Rust
dependency-scope setting on its Rust-specific path. Introduce an explicit TS run closure
or small language dispatcher without a general plugin registry. Reuse Task 3 reports;
Rust may have empty notes. Each side groups Rust separately and TS/JS together, runs
bounded groups, merges graphs with validated identities/targets, and returns per-side notes.
Preserve before/after worker ownership, peer cancellation and original failure causes.

Update extension routing/coverage for each side; tests-excluded supported files remain
analyzed. Empty language groups do not launch missing tools. Synchronize StructureComparison
projection, src/structure-review/types.ts and test fixtures. Cache keys and tests toggle
must keep existing lifecycle behavior; fixed tools are captured by bootstrap/run settings.

Update Rust-only loading/unsupported strings and render concise per-side analysis notes
using existing shadcn/base primitives where appropriate. Do not display raw server internals
or graph-external targets. Preserve import/export objects and JSX relationship semantics,
multi-declaration cards and original-source detail language. Add a reusable Preview Harness
scenario with mixed TS/JS/Rust graph, long names/notes/JSX source and include-tests state.

Focused tests cover mixed dispatch and TS↔JS Node targets, merge collision, empty sides,
extension changes, failure/cancellation and note separation from diff, frontend notes,
language wording, JSX relation display, duplicate declaration cards, tests/cache/stale results.
Use in-app browser to verify wide/narrow layout, long content, details/controls/notes; restore
viewport. Run frontend focused tests, lint/typecheck/build. Do not manually trigger quality-gate.
Commit and report visual evidence paths plus test commands/results.

## Task 5: Whole-feature acceptance and documentation

The controller verifies all previous task reports and runs the final required checks once:
Cargo fmt/check/clippy as appropriate and full --all-targets test + doc tests through the
repository lock, fixed-server acceptance, frontend full tests/lint/build and script tests.
Do not repeat already successful large checks without new changes/concerns. Review the
entire branch with an independent reviewer on the most capable available model.

Complete tool setup/usage documentation in README/spec index, record actual compatibility
and scope behavior, and mark implementation status truthfully. Keep research experiments
separate from production verification; do not claim dynamic analysis or OS sandbox guarantees.
Fix all material findings through the assigned implementer and scoped re-review. Preserve
main's preexisting .gitignore change and research files; no unrelated edits, pushes or merges.

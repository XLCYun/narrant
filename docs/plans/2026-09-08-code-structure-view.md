# Code Structure View implementation

Spec: [0018-code-structure-changes](../specs/0018-code-structure-changes.md), accepted by the user on 2026-09-08.
Architecture: [ADR-0030](../adr/0030-encapsulate-code-structure-analysis.md).
Workspace: `/Users/lizhanfa/projects/narrant/.worktrees/ast-structure-review`, branch `codex/ast-structure-review`, base `da4f583089350b6fe8632fb83dcc9d7558e77286`.

## Global Constraints

- All work stays in this isolated worktree. Do not merge, push, close Issues, or change the user's other checkout from a task.
- The accepted specification governs scope. Do not silently reduce native declarations, common cross-file calls, change classification, or full-file Peek.
- One analysis module hides parsers and language rules; UI consumes typed Snapshot-bound results. Before uses fixed Comparison Base and after fixed Head, each with its own imports and configuration. Read Git objects; do not add a clone or checkout for analysis.
- Structure absence, relationship absence, unavailable source, unsupported input, incomplete analysis, candidates, and unresolved targets are distinct facts. Unknown is never deletion. Before/after relations form a union; relationship-only changes remain discoverable.
- First release supports TS/JS including TSX/JSX, Rust, and the corresponding common module extensions. No language-server installation or lifecycle, no dependency-source scanning, no hidden runtime grammar download.
- Use Chinese comments for new functions, types, and classes. Tests stay in separate files; Rust tests live under the production module's `tests/` directory.
- Run Cargo only through `scripts/cargo-agent.sh` using the actual supported Cargo arguments, normally `scripts/cargo-agent.sh check --manifest-path src-tauri/Cargo.toml --lib`. Never share a target directory with another worktree or turn off incremental. Use focused tests during changes; coordinate one full Rust test run per completed Issue with the controller.
- Use existing shadcn components before custom controls. Meaningful automated tests, typecheck, lint, production build, and real in-app browser verification are required for final UI acceptance. Use `pnpm dev:preview` when Tauri state prevents browser rendering; do not modify live business data for preview. Do not manually trigger quality-gate.
- Each task self-reviews, records exact validation commands/results, and commits only its files. The controller schedules an independent task review and final branch review.

## Task 1: Validate and integrate a reusable parser foundation

Read the specification's dependency investigation, native structures, and failure semantics, plus ADR-0030 and repository AGENTS. Work in the workspace above.

Validate the current Rust APIs of `tree-sitter-language-pack` and `ast-grep-language` / `ast-grep-outline` using authoritative docs and actual candidate package source. Prefer reusable extraction and statically bundled TS/TSX/JS/Rust grammars; choose based on observed coverage and integration feasibility. Freeze chosen dependency versions in Cargo.lock. Do not invent provider capabilities or promise a complete call graph from a syntax package.

Add a focused internal parser foundation under `src-tauri/src/review/structure/`, integrated into the existing Rust module tree. Prove in-memory parsing of TS/TSX/JS/Rust and enumeration of native declaration kinds, names, ranges, and ERROR/MISSING information through a small owned internal contract. Do not implement the whole matcher or renderer here. Design the interface to retain source positions and provider information internally so later language extraction can supplement real gaps without reparsing in UI. Keep production files cohesive and split by responsibility as needed.

Create meaningful public-boundary parser tests in `structure/tests/`, covering a TS class/function/arrow, TSX function component, Rust struct/trait/impl/method, and damaged input. Test real parser outputs rather than mocking the provider. Verify the chosen grammars operate with runtime downloading disabled/unavailable; do not claim a network-isolated run unless actually exercised. Use Cargo wrapper and report any environment limitation precisely.

Write `docs/research/code-structure-parser.md` recording pinned dependency choice, candidate comparison, exact observed extraction gaps to implement in Task 3, offline packaging evidence, and test commands. Supply later tasks the owned parser interface and unresolved technical risks. Commit this foundation; report the full details to the task report file supplied by the controller.

## Task 2: Read complete immutable Snapshot source trees

Extend Review content access so the structure module can enumerate and read supported files at fixed Comparison Base and Head, including unchanged files and project configuration. Read tree/blob objects from the existing repository through its Git process adapter; no second checkout, moving ref reads, or filesystem fallback. Preserve rename and true absence, binary/symlink/submodule modes, unavailable objects, cancellation, and text decoding semantics. Keep existing File Change content behavior compatible.

Expose a small owned source interface usable by analysis and full-file Peek, including reading unchanged context and either real file location of a moved declaration. Use NUL-delimited machine Git output and fixed object identities; batch reads where it materially avoids per-file process overhead. Tests use real temporary repositories for unchanged files, deleted files, rename, moving refs after Snapshot creation, invalid/unavailable objects, and cancellation. Follow existing Git testing patterns and new test layout rules. Commit focused implementation and tests; coordinate the Issue-level full test with controller.

## Task 3: Extract native declarations and lexical evidence

Build on Task 1 parser and Task 2 source contract to extract all structures required by spec A2/A8/A9: types, methods/fields, independent/named arrow functions, enums, top-level constants, module/namespace containers, nested named functions, Rust impl groups and trait identities. Preserve multiple source locations and semantic parent identity. Aggregate same Rust type across impl files while preserving distinct trait method identities.

Extract signatures, owned body tokens, associated comments, lexical scopes, imports/exports/aliases, call sites and type/implementation evidence for later matching/resolution. Named descendants must not cause duplicate parent modifications; anonymous callbacks remain within owned body changes but their call-site execution context is retained. Parse recovery provides partial results with affected ranges and coverage; damaged scope cannot establish absence. Do not use whitespace stripping to normalize literals or ASI-sensitive syntax. Apply provider rules first, supplement actual missing language behavior internally.

Test through the module boundary with representative TS/JS/TSX/Rust fixtures, Chinese text/CRLF, same-name traits, multi-file impls, nested callbacks, formatting/literal distinctions, and ERROR/MISSING. Record unsupported/dynamic facts as evidence, not fabricated resolved declarations. Commit focused extraction modules and tests.

## Task 4: Match declarations and classify structural changes

Match two extracted sides conservatively. Stable identities survive reliable rename/move; uncertain multiple candidates remain add/delete only with sufficient analysis coverage. Support simultaneous rename/move/body/signature/documentation labels and count changed declarations once. Parents show contained change counts separately from their own change. Ignore only safe formatting; strings/raw strings/templates and ASI-sensitive newlines retain meaning. Never infer deletion from failed or partial parse.

Define the owned analysis result model needed by transport/UI: native kinds, before/after sources and signatures, parent/impl groups, change labels, coverage and explicit unknown facts. Retain deleted and moved locations. Tests cover duplicate names/bodies, overloads, moved+renamed+edited functions, only comment changes, parent/child double counting, incomplete sides, and same-name distinct trait methods. Commit matcher and meaningful behavioral tests.

## Task 5: Resolve relationships and their changes

Implement built-in relationship analysis against each side's complete repository index and project context. Required common cases: lexical named calls and shadowing, explicit imports/aliases and Narrant TS path aliases, Rust module/use paths, qualified static/associated calls, clear this/self targets, ownership/inheritance/trait implementation, type references, and imports. Preserve resolved/candidate/unresolved distinctions, evidence and original call sites, with no runtime completeness claim for dynamic/reflective/complex type dispatch.

External targets expose names/package identity without scanning dependency source. Build before/after union relations using matched endpoints and kinds, aggregate repeated call sites without treating moved line numbers as edge changes. Unknown targets/coverage cannot create false removals. Relationship-only changes remain discoverable and unchanged endpoints remain context. Tests cover required positive/negative cross-file examples, old/new context, candidate ambiguity, alias shadowing, anonymous callback origin, same endpoints with moved lines, unknown transitions and external packages. Commit resolver and tests.

## Task 6: Integrate analysis lifecycle and typed application transport

Connect the analysis module to existing Review runtime and typed Tauri transport/application adapters. One request owns source discovery, parse/index/match/relations and Snapshot identity; UI does not orchestrate stages. Analyze in the background with progress, coverage, cancellation and retry while file reading remains responsive. Old asynchronous results must not replace results after refresh, cancellation, review closure or repository deletion. Preserve each open Review's presentation state, with no cross-process graph history or object pinning.

Provide typed frontend contracts for graph result, status and full-file Peek source reads including real file-change metadata, unchanged context, before/after availability and independent selection. Check existing commands/DTO conversion/concurrency tests, add targeted lifecycle and transport boundary tests, and keep existing APIs compatible. Optional analysis failure must not roll back Review refresh. Commit backend and frontend application integration.

## Task 7: Render the Code Structure View

Add the third peer Review View and a real navigable graph using the typed result. Retain native structures and collapsible members/impl groups, module/file grouping and automatic layout. Initially show changes (including relationship-only changes) and one-hop old/new context; expand upstream/downstream, search all analyzed nodes and filter declaration/change types. Large results load display batches with visible counts and no silent omissions.

Provide relationship type/range controls: inheritance/implementation visible; calls selected node one hop by default, current-graph/all-hidden options; type/import default off; candidates default hidden; external nodes hideable. Show added/deleted/retained relations using distinct colors with legend/direction and accessible non-color signals, supporting light/dark themes. Graph view and selection persist per open Review and Snapshot; resetting invalid state after refresh must follow Task 6.

Use shadcn controls and existing UI language/appearance patterns. Add representative reusable Preview Harness scenarios including empty/loading/partial/cancelled/error/relationship-only/large/long-label states. Component/graph-model tests verify visible behavior and topology selection without mirroring production implementation. Conduct real browser checks at explicit wide/narrow viewports and restore viewport. Commit graph, integration and tests; supply screenshots or inspection evidence.

## Task 8: Add full-file Structure Peek

Implement bottom resizable Peek with the existing Monaco full-file Diff, never method slicing. External reveal requests wait for model/diff readiness and use true file lines (Unicode/CRLF included). Default new side except deleted old; relationship clicks reveal call/reference occurrence with a multiple-location list. Actual file rename follows real oldPath/path; cross-file symbol move switches between each file's real full Diff, without a fake combined File Change.

Aggregate type opens declaration by default and lists impl sources; member goes directly to own file. Unchanged context is readable without creating a File Change, unavailable source is explicit rather than empty, external nodes have no fake definition. Peek state is independent of File Changes selection; explicit open-in-File-Changes action only for actual changes. Preserve graph viewport, return focus on keyboard close, narrow single-pane mode and existing wide diff preferences.

Add meaningful component/editor integration tests and reusable preview states. Real browser acceptance covers wide/narrow, long/short source, changed/deleted/moved sources, source menu, resizing, switching selections, close/focus and loading/unavailable states. Commit Peek, editor navigation changes and tests.

## Task 9: Validate Narrant coverage, performance and final integration

Run the complete accepted A1–A18 scenarios and record real outcomes. Use fixed Narrant revisions and the full tracked supported-source scope including the large generated icon manifest; record source counts, machine/build mode, cold/warm timing, first results/full analysis, peak memory and cancellation/graph/search/Peek behavior. Do not reduce scope to hit a target or fabricate an unmeasured budget. Resolve observed correctness or interaction failures.

Complete frontend tests/typecheck/lint/production build and the coordinated final Rust tests through the wrapper. Run real in-app browser Preview Harness acceptance across relevant widths, light/dark themes, long content, relationship controls, partial coverage and full-file Peek; inspect screenshot, DOM and critical bounds, then restore viewport. Write verification evidence and material limitations to `docs/research/code-structure-validation.md`. No manual quality-gate, shared-branch merge or publication. Commit fixes and evidence for whole-branch review.

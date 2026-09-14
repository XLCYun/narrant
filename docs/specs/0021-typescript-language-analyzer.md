---
status: implementation
date: 2026-09-15
---

# TypeScript / JavaScript Code Graph

## Scope and authority

Implement the scope accepted in the [design discussion](../research/typescript-code-graph-plan.md)
and validated by the [LSP experiments](../research/typescript-lsp-validation.md).
This spec extends [0018](0018-structure-review-code-graph.md) and
[0020](0020-structure-changes-review-view.md); their graph, comparison and workspace
lifecycle contracts remain binding. TypeScript is a peer of the existing Rust adapter.

The user has authorized implementation. No further product confirmation is required.
Use Oxc for syntax and own-content extraction and fixed TLS 6.0.0 + tsserver 6.0.3
for semantic evidence, through the existing shared LSP transport. Do not replace this
with a Compiler API graph extractor, tree-sitter, or a regex-based parser.
Using the fixed TypeScript library to read JSONC/project configuration is permitted;
semantic graph queries stay in the language service.

## Language and objects

- Cover `.ts`, `.tsx`, `.js`, `.jsx`, `.mts`, `.cts`, `.mjs`, `.cjs` and declarations
  (`.d.ts`, `.d.mts`, `.d.cts`) together. TS and JS share semantic projects but keep
  appropriate graph/editor language labels. `.js` can contain JSX, as in tsserver.
- Extract modules/namespaces (Container); classes/interfaces/type aliases/enums (Type);
  functions/methods/constructors/accessors (Callable); variables/fields/properties/enum
  members (Value); top-level executable statements and static blocks (ExecutionRegion);
  import/export/re-export declarations (Extension).
- A direct const identifier binding with an arrow initializer is one Callable and no
  duplicate Value. Unwrap transparent parentheses, assertions, non-null and satisfies
  for classification only; preserve their semantic text. Inline callback arrows stay
  in their nearest modeled owner's own code and never create anonymous graph nodes.
  Call/factory/conditional/destructuring/let/var forms are not promoted by this rule.
  Explicit named declarations inside callbacks still follow ordinary declaration rules.
- Keep every source declaration of a merged interface/namespace in its own file and
  containment tree. Do not combine cross-file fragments or choose the first definition
  arbitrarily. Same-owner function overloads form one Callable, preserving signature
  order and the implementation separately.
- Preserve original source, including JSDoc. Reuse the language service's native
  JSDoc inference, without a separate JSDoc parser, contract model or diff pipeline.
- Test files are supported extensions whose basename ends in `.test.<ext>` or
  `.spec.<ext>`, or whose path has a `__tests__` directory segment. With include_tests
  false they contribute no nodes/outgoing relationships; unknown/shared files remain.
  Calls named test/it/describe do not exclude a mixed production file.

## Content and identities

Use the existing six variants and public graph validation. Stable identity uses the
language, repository-relative path, lexical owner and source declaration identity;
never use line offsets, absolute checkout/analysis paths, or tsserver symbol numbers.
No rename/move inference. Selection changes must not change an existing object's ID.

Signature and code represent only the object's own content. A child method edit must
not change its containing class's own content; an inline callback edit must change its
owner. Source is complete original display text and does not participate in diff.
Normalization ignores formatting and ordinary comments but preserves ASI behavior,
template/regex values and meaningful JSX text. JSDoc can affect native semantic
relationships without becoming its own signature/contract entry.

Oxc byte spans and LSP UTF-16 positions must be converted against the same source,
validating UTF-8 boundaries and handling CRLF, Unicode paths and non-BMP characters.

## Relationships

Extract imports/re-exports, statically confirmed calls (including new where modeled),
writes, function-value references, inheritance/implementation and field/property type
relationships consistent with 0018 and the existing Rust scope. Keep a syntax occurrence
even if call hierarchy omits it. Do not turn an import alias or a type-only signature into
a definite runtime call. Dynamic/ambiguous targets remain Unresolved. Named function declarations in repository
`.d.ts`/`.d.mts`/`.d.cts` files and explicit `declare function` declarations (including
legal ambient contexts) introduce function-value bindings and may be static call targets:
selected declarations are Node, unselected declarations are Repository External. Bare
structural call signatures/interface methods and isolated non-ambient overload signatures
without an implementation do not establish runtime call targets. These edges identify
verified source declarations; they do not assert that ambient declarations match a deployed
runtime implementation.

Custom JSX occurrences use `jsx.uses_component`, distinct from `code.calls`. Intrinsic
tags such as div/button have no nodes or component edges. Expressions and callbacks
inside their attributes still contribute source content and ordinary relationships.
Deduplicate high-level relation facts; duplicate LSP ranges cannot change the diff.

Resolved selected declarations use Node. Resolved declarations outside the graph use
External with stable identity and readable repository/dependency scope. Missing or
conflicting targets use Unresolved. UI continues hiding graph-external targets.

## Project input and defaults

Default semantic context is this side's repository plus standard libraries. ECMAScript
libraries follow effective target/lib; DOM/WebWorker follow project configuration;
Node runtime declarations come from a fixed allowed package closure (initial profile:
@types/node 22.20.1 and undici-types 6.21.0). Do not infer project runtime version from
the Node executable running TLS. Respect explicit noLib and repository declarations.

Third-party business sources/declarations are not provided to default semantic projects.
Disable ATA, project plugins and arbitrary automatic @types. Missing business dependencies
can yield reliable partial semantics and Unresolved references; do not auto-install or
build the reviewed project. Necessary configuration/package metadata may be read.

Prepare a temporary analysis directory preserving first-party relative layout, effective
tsconfig/jsconfig configuration, extends, references and package metadata. Recreate only
first-party workspace links. Do not copy the current checkout's node_modules tree or
silently substitute arbitrary source entries for package exports. Do not flatten all
projects into a single noResolve Program: that fails source redirects in composite projects.

Normalize config paths into the view while preserving module/moduleResolution, paths,
imports/exports, package type and extension substitution. Support TS5/6 representative
configurations without upgrading the reviewed source/config. Surface compatibility
diagnostics and missing contexts; an expected deprecation alone is not unavailable
semantic analysis. Config-less selected sources get an explicit, deterministic inferred
project profile, documented in the implementation; do not alter actual repository files.

All input paths must belong to the allowed repository/view or fixed runtime context.
Check the service's actual project identity and membership before accepting results;
unexpected outside inputs must not become a successful graph. This is a semantic scope
contract, not an OS-level sandbox for arbitrary hostile language-server inputs.

Register projects before didOpen, use `useSyntaxServer: never`, and explicitly select
and validate a project's real name for multi-project queries. TLS's public tsserverRequest
extension is allowed. An invalid requested project must not silently fall back to another.
One source declaration has one graph identity even when several projects include it;
conflicting target evidence stays uncertain and produces an analysis note.

## Tools and lifecycle

Use a fixed external toolchain, matching the Rust development tool convention. The
application does not download tools during analysis. Provide an explicit development
setup command with pinned versions/integrities and clear missing-tool errors.
Formal bundled runtime distribution remains deferred.

The TS adapter owns its temporary analysis directory and all child processes. Use the
existing CancellationCheck and process lifetime guards. Cancellation, timeout, errors
and panic must reclaim child work before returning/releasing the workspace lease.
Shutdown acknowledgement alone does not prove the tsserver child exited.

Preserve existing before/after parallelism and fixed side snapshots. Run each side's
language groups with bounded concurrency. A hard failure in any language fails the
whole comparison and cancels the peer; do not publish a successful empty/partial graph
to conceal a failed service. Empty/fully excluded selections need no server startup.

## Application and UI

Route every side independently by supported extension; TS/JS stay in one group so
cross-language first-party references become Node correctly. Merge validated language
graphs without identity collisions. Coverage retains analyzed/absent/unsupported-kind/
unsupported-language; excluded tests are still analyzed supported inputs.

Report per-side analysis notes separately from semantic nodes and differences. Missing
configuration/dependencies and conflicting contexts must be understandable; raw tsserver
internals are not product controls. Synchronize Rust projection and frontend types.
Retain existing tests toggle, cache/stale-result protection, external-reference hiding,
single-card declaration placement and original-source details. Update Rust-only wording.

## Acceptance

Run focused syntax/graph/diff tests; real fixed-service tests for scope, aliases, JS/JSX,
Node ESM/CJS, references with missing declaration outputs, workspace package names,
Node libraries, JSDoc, overload/merged declarations, unresolvable/dynamic calls and
Unicode. Test cancellation and tool failures. Verify mixed Rust+TS/JS application
composition, empty sides, extension changes, coverage and per-side notes.

Keep meaningful own-content tests for nested declarations, anonymous callbacks,
format-only changes, ASI, templates/regex/JSX and stable identity across checkout roots.
Use separate module tests/ files with readable Chinese comments.

All Cargo commands go through scripts/cargo-agent.sh and its shared lock; retain a
separate target per worktree. Focused tests during implementation, one full Rust suite
before completion; rerun only affected tests after review fixes unless shared contracts
change. Run frontend tests, type checking, lint and production build. Use the in-app
browser and Preview Harness for UI changes, including wide/narrow/long-content states.
Do not manually trigger quality-gate.

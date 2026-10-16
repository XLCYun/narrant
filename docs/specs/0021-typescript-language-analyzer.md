---
status: accepted
date: 2026-09-15
---

# TypeScript / JavaScript Code Graph

## Scope and authority

Implement the scope accepted in the [design discussion](../research/typescript-code-graph-plan.md)
and validated by the [LSP experiments](../research/typescript-lsp-validation.md).
This spec extends [0018](0018-structure-review-code-graph.md) and
[0020](0020-structure-changes-review-view.md); their graph, comparison and workspace
lifecycle contracts remain binding. TypeScript is a peer of the existing Rust adapter.

The current implementation follows [ADR-0035](../adr/0035-migrate-existing-analyzers-to-native-apis.md)
and [0025](0025-migrate-existing-analyzers-to-native-helpers.md). `native-helpers/typescript`
uses fixed Node 24.18.0 / TypeScript 6.0.3 and the in-process Compiler/Language Service API
for syntax, own-content and definition evidence. One worker handles one fixed side; the
common Native Host owns process lifetime and graph validation. Oxc, TLS, tsserver, old
setup scripts and the LSP transport are retired; no server fallback is retained. The
unpublished 1.1.0 candidate has passed local macOS ARM64 checks and the final GitHub-hosted
macOS ARM64 / Windows x64 helper, installation, Review and public graph gates. See
[final migration evidence](../research/existing-native-helper-migration-validation.md) and
[earlier TypeScript evidence](../research/0025-typescript-native-helper-migration.md).

## Language and objects

- Cover `.ts`, `.tsx`, `.js`, `.jsx`, `.mts`, `.cts`, `.mjs`, `.cjs` and declarations
  (`.d.ts`, `.d.mts`, `.d.cts`) together. TS and JS share semantic projects but keep
  appropriate graph/editor language labels. `.js` can contain JSX, as in tsserver.
- Extract modules/namespaces (Container); classes/interfaces/type aliases/enums (Type);
  named functions/methods/constructors/getters/setters (Callable); class fields (including
  constructor parameter properties), interface/type-literal properties, index signatures
  and enum members (Value);
  top-level executable statements and static blocks (ExecutionRegion).
- Every selected file with graph objects has one file Container (`native_kind: file`),
  named by its repository-relative path with `/` separators on every platform.
  It contains the file's top-level declarations and
  execution region; explicit namespaces/modules retain their own nested containment.
  The file container organizes source ownership, including script/declaration files,
  without changing the language service's module/global-scope semantics. It contributes
  no signature or own code to diff; full file source is available only for display.
  Empty, imports-only and excluded test files do not create empty containers.
  Explicit language modules and namespaces retain `module` and `namespace` kinds;
  directories are presentation groups and are not emitted by language analyzers.
- Initializers, ordinary top-level statements and directives share one ExecutionRegion
  per file or explicit namespace, in source order; no individual `statement` or
  `directive` nodes are emitted. Functions and class static blocks keep their own
  execution ownership. Classes, interfaces, enum bodies, explicit namespaces/modules
  and type aliases' own object types exclude independently modeled members from their
  AST member lists before Oxc generates comparison text. Adding/removing or reordering
  those members changes containment; reordering them alone is not a diff. Neither
  operation changes the container's own content.
  Unmodeled members and declaration headers remain in the comparison.
- Initializers, parameter defaults, superclass expressions, function implementations
  and top-level execution expressions are compared in full, even when they contain
  independently modeled functions or classes. Inserting or reordering a named function
  in a field's array/object initializer changes both the field's content and, where
  applicable, its containment. Expression lists are never stripped or repaired manually;
  array holes, spreads, argument order and literal text retain their meaning.
  Mixed variable statements inside namespaces remove only independently modeled bindings,
  preserving any remaining ordinary values. Projection affects comparison only, leaving
  source excerpts, node identities and language-service positions unchanged.
- Ordinary `const`/`let`/`var` and destructuring declarations never create Value nodes,
  at either top level or inside functions, loops or callbacks. Ordinary object-literal
  properties/shorthands and auto-accessor properties also create no nodes.
  Their code inside a modeled owner still participates in that
  owner's content comparison. Calls in omitted top-level initializers share their scope's
  ExecutionRegion with ordinary executable statements; calls inside functions/callbacks
  belong to the enclosing modeled owner.
- Constructor [parameter properties](https://www.typescriptlang.org/docs/handbook/2/classes.html#parameter-properties)
  introduced by `public`, `private`, `protected`, `readonly`, or `override` (including
  legal combinations) are Value nodes with `native_kind: field`, directly contained by
  their modeled class. They share ordinary instance fields' owner/name identity, rather
  than becoming constructor children. Preserve modifiers, decorators, optional markers,
  type annotations, defaults and original parameter source. Field type uses and calls
  within defaults belong to the field; constructor signatures still compare the complete
  ordered parameter list and defaults. Ordinary and rest parameters create no fields.
  A function-valued default does not prove the property's runtime callable identity,
  since a caller may supply a different argument.
- Property and index-signature Value nodes must be direct members of a named interface
  body or a type alias's own object type, outside every function/callback scope.
  Parentheses, unions and intersections may wrap the alias's object type; generic
  arguments, arrays, nested property types and function types do not establish membership.
  Inline parameter/return types, `useState<{ ... }>()` and members of function-local
  interfaces/type aliases create no Value nodes. Their type text remains in the owning
  declaration's content comparison. Existing local Type nodes are retained.
  A function-typed property such as `interface C { run: () => void }` remains a Value.
- Import/export/re-export statements, including `import x = require(...)`, do not
  create graph nodes or import/re-export edges. Keep their module paths, lexical
  bindings and alias query locations internally so calls still resolve across files
  and barrel exports. Export wrappers keep each retained declaration as a single real
  node, with export modifiers in its signature and original source. Executable export
  expressions retain an execution owner; CommonJS require calls use the same execution
  ownership rules, without creating variable nodes.
- A simple `const`/`let`/`var` binding directly initialized with an arrow or function
  expression is one Callable, using its binding name and no duplicate Value or inner-name
  node. Direct function-valued class fields and statically named object properties are
  Callables too. Unwrap transparent parentheses, assertions, non-null and satisfies for
  classification only; preserve their semantic text. Call/factory/conditional/destructuring
  forms are not inferred as functions.
- Only named Callables outside every function/callback body are modeled. This includes
  namespace functions, class methods, function-valued fields and top-level object methods.
  Any nested function declaration/expression or callback stays in its enclosing owner's code;
  its outgoing calls are still collected. Anonymous structural call/construct signatures
  do not create Callable nodes. Omitted object wrappers retain lexical identity metadata
  so equally named methods in different objects keep distinct stable IDs.
- Keep direct immutable variable alias metadata internally for LSP definition continuation;
  removing a variable node must not break reliable calls through const alias chains.
- Keep every source declaration of a merged interface/namespace in its own file and
  containment tree. Do not combine cross-file fragments or choose the first definition
  arbitrarily. Same-owner function overloads form one Callable, preserving signature
  order and the implementation separately.
- Preserve original source, including JSDoc. Reuse the language service's native
  JSDoc inference, without a separate JSDoc parser, contract model or diff pipeline.
- Test files are supported extensions whose basename ends in `.test.<ext>` or
  `.spec.<ext>`, or whose repository-relative path has a `__tests__` directory segment.
  Checkout-root names and ancestor directories outside the repository do not affect this
  classification. With include_tests
  false they contribute no nodes/outgoing relationships; unknown/shared files remain.
  Calls named test/it/describe do not exclude a mixed production file.

## Content and identities

Use the existing six variants and public graph validation. Stable identity uses the
language, repository-relative path, lexical owner and source declaration identity;
never use line offsets, absolute checkout/analysis paths, or tsserver symbol numbers.
No rename/move inference. Selection changes must not change an existing object's ID.

Signature and code represent the object's own content under the declaration-member
and complete-expression rules above. A child method edit must not change its containing
class's own content; a function embedded in a field initializer remains part of that
field's content even if also modeled independently. Source is complete original display
text and does not participate in diff.
Normalization ignores formatting and ordinary comments but preserves ASI behavior,
template/regex values and meaningful JSX text. JSDoc can affect native semantic
relationships without becoming its own signature/contract entry.

Native TypeScript AST spans and definition positions refer to the same SourceFile text.
The helper preserves original Unicode/CRLF source and translates only at its own native
API boundary; Host receives final normalized objects, not LSP positions. The unused Host
UTF-16 SourceText index and its transport tests have been removed.

## Relationships

Extract statically confirmed calls (including new where modeled),
function-value references, inheritance/implementation and field/property type
relationships consistent with 0018 and the existing Rust scope. Keep a syntax occurrence
even if call hierarchy omits it. Do not turn an import alias or a type-only signature into
a definite runtime call. Dynamic, unresolved or untrusted definition evidence remains
Unresolved. Named function declarations in repository
`.d.ts`/`.d.mts`/`.d.cts` files and explicit `declare function` declarations (including
legal ambient contexts) introduce function-value bindings and may be static call targets:
selected declarations are Node, unselected declarations are Repository External. Bare
structural call signatures/interface methods and isolated non-ambient overload signatures
without an implementation do not establish runtime call targets. These edges identify
verified source declarations; they do not assert that ambient declarations match a deployed
runtime implementation.

Call edges follow the language service's `definition` source-navigation result rather
than enumerating possible runtime implementations. A single verified, eligible target
produces an edge, including a union member call where TypeScript navigates only to
`A.method` for `A | B`. Do not supplement candidates with `typeDefinition`. Multiple
distinct candidates returned by definition, conflicting project results, unresolved
aliases or untrusted evidence remain Unresolved. Existing declaration eligibility,
alias-following and type-only-import checks still apply.

Omit `code.reads` and `code.writes`, including simple/compound assignments and update
expressions. Keep traversing their expressions for calls and function-value references
under the existing ownership rules; confirmed data accesses must not be relabeled as
`code.references`. Field/property type relationships are unchanged. Assignment and
update text remains part of its owner's normalized code and original source, so these
edits still participate in content comparison without a separate read/write edge.

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

All language analyzers must read the same Git workspace supplied for that comparison
side. Reuse the Review head checkout and the single before checkout under the existing
lease. Do not make per-language source trees, copy source/configuration/declaration files,
or create physical workspace package links. The TS adapter may create temporary files
only for its own trusted helpers, launcher and control metadata; it does not own or delete
the Git workspace.

Keep the first-party source inventory as paths, separate from loaded text. Preparing
project registrations and file associations must not eagerly read every source or build
its line index. Load selected source text through one cache shared by AST extraction and
didOpen; load other source/declaration text and ASTs only when a semantic query needs
them. File provenance uses the inventory even before a file is cached. tsserver may
still load project files for its own semantic analysis.

The caller keeps the input workspace stable for the analysis lifetime, as specified by
AnalysisInput. Review owns checkout lifetime and revision/content checks. The TS adapter
must not reread the entire source inventory before and after analysis to compare snapshots.
Retain path scope, helper output consistency, service project and source position checks;
deduplicate preparation paths before checking them.

Checks belong to the layer receiving new input. Cache each confirmed project's actual
member set for the fixed workspace session; subsequent queries check the explicit project
name and file membership in memory, without `projectInfo` calls before and after every
definition request. Project discovery seeds this same cache. Keep diagnostics per
file/project, including empty results, so repeated queries need no extra validation or RPC.
Native definition candidates pass helper source-scope and same-SourceFile range checks.
Downstream declaration matching trusts the checked path and native span. Identity encoding
is lexical; cached source is reused within the fixed request, and uncached inputs still
require provenance checks. No LSP RPC or Host position index remains.

Preserve effective tsconfig/jsconfig, extends, references and package module metadata
through a read-only TypeScript file host. Configuration overrides and first-party package
lookup aliases exist only in memory. Read standard/runtime declarations directly from the
fixed tool directory. Do not silently substitute source entries for package exports or
flatten projects into one noResolve Program: that fails composite source redirects.

Normalize config paths within the same workspace while preserving module/moduleResolution, paths,
imports/exports, package type and extension substitution. Support TS5/6 representative
configurations without upgrading the reviewed source/config. An expected deprecation
alone does not make semantic analysis unavailable. Config-less selected sources get an explicit, deterministic inferred
project profile, documented in the implementation; do not alter actual repository files.

All input paths must belong to the allowed Git workspace or fixed runtime context.
Check the service's actual project identity and membership before accepting results;
unexpected outside inputs must not become a successful graph. This is a semantic scope
contract, not an OS-level sandbox for arbitrary hostile language-server inputs.

Register projects before didOpen, use `useSyntaxServer: never`, and explicitly select
and validate a project's real name for multi-project queries. TLS's public tsserverRequest
extension is allowed. An invalid requested project must not silently fall back to another.
One source declaration has one graph identity even when several projects include it;
conflicting target evidence stays unresolved.

## Tools and lifecycle

Use fixed tools supplied to the adapter; the adapter does not download them during
analysis. Production now uses the shared TypeScript / JavaScript
[Language Support Pack](0022-language-support-packs.md), containing Node and the
fixed native TypeScript library/runtime declarations. The application prepares this pack
before analysis. Development tests use a locally built candidate via the normal builder.
Neither preparation nor analysis installs dependencies into the reviewed project.

The common Native Host owns scratch/control data and `child_process::ProcessTree`.
The application uses `native_analyzer::CancellationCheck`; the helper controls its worker
thread and waits for it before terminal output. Cancellation, timeout, errors and panic
reclaim child work before returning/releasing the workspace lease. A completed message
alone does not prove the process tree exited.

Preserve existing before/after parallelism and fixed side snapshots. Run each side's
language groups with bounded concurrency. A hard failure in any language fails the
whole comparison and cancels the peer; do not publish a successful empty/partial graph
to conceal a failed service. Empty/fully excluded selections need no helper startup.

## Application and UI

Route every side independently by supported extension; TS/JS stay in one group so
cross-language first-party references become Node correctly. Merge validated language
graphs without identity collisions. Coverage retains analyzed/absent/unsupported-kind/
unsupported-language; excluded tests are still analyzed supported inputs.

Language analyzers return CodeGraph directly. The application and frontend do not
collect, transport or display analysis notes. Missing optional context and conflicting
project targets retain the existing unresolved-reference behavior; hard service,
configuration-boundary and lifecycle failures still fail the comparison explicitly.
Only semantic diagnostics required to reject type-only values used as runtime functions
are queried internally; compiler-option and syntax diagnostics are not collected for display.
Retain existing tests toggle, cache/stale-result protection, external-reference hiding,
single-card declaration placement and original-source details. Update Rust-only wording.

## Acceptance

Run focused syntax/graph/diff tests; real fixed-service tests for scope, aliases, JS/JSX,
Node ESM/CJS, references with missing declaration outputs, workspace package names,
Node libraries, JSDoc, overload/merged declarations, unresolvable/dynamic calls and
Unicode. Verify actual project members and definition URIs point to the original Git
checkout or fixed standard libraries, the workspace tree/content remain unchanged,
and helper cleanup never removes the workspace. Test cancellation and tool failures. Verify mixed Rust+TS/JS application
composition, empty sides, extension changes and coverage.

Keep meaningful own-content tests for nested declarations, anonymous callbacks,
format-only changes, ASI, templates/regex/JSX and stable identity across checkout roots.
Use separate module tests/ files with readable Chinese comments.

All Cargo commands go through scripts/cargo-agent.sh and its shared lock; retain a
separate target per worktree. Focused tests during implementation, one full Rust suite
before completion; rerun only affected tests after review fixes unless shared contracts
change. Run frontend tests, type checking, lint and production build. Use the in-app
browser and Preview Harness for UI changes, including wide/narrow/long-content states.
Do not manually trigger quality-gate.

## Historical implementation verification (before the 0025 migration)

The application implementation was completed and accepted on 2026-09-15. Production
adapter fixtures on macOS exercise the fixed TLS 6.0.0 and tsserver 6.0.3 services,
including modern NodeNext and Bundler projects, JS/JSX, Node ESM/CJS wrappers, Promise,
native JSDoc, project references, workspaces, missing business dependencies, cancellation
and process cleanup. The TS 5-style fixture uses CommonJS, legacy Node module resolution,
baseUrl and paths under the fixed TS 6 engine. It retains a reliable local call, leaves a
conflicting import Unresolved and reports the native deprecation note. This is representative
configuration evidence, not execution with a TS 5 engine or a blanket TS 5/6 compatibility
claim.

Final verification used Node 24.18.0 and the repository Cargo lock wrapper. The all-targets
Rust suite passed 650 tests with 45 opt-in tests ignored; doc tests completed successfully
with no examples. Relevant real-service tests were then run separately against that final
test binary: the Rust Review lifecycle fixture passed with four before nodes, four after nodes
and six changes, and the TS/JS lifecycle-to-DTO fixture passed with a real cross-language call,
original source, diff, TS2307 notes, coverage and workspace cleanup. The final all-targets
Clippy check passed with warnings denied, and rustfmt check passed.

A subsequent focused final-review correction preserves native `${configDir}` expansion at
its consuming configuration, including inherited scalar/list paths and file patterns.
Real-filesystem configuration regressions compare fixed-TS effective options, membership
and alias resolution before/after preparation, and reject expanded paths or symlink
ancestors escaping the repository. Actual fixed-service and public-graph fixtures
retain the exact first-party declaration and definite local call despite a competing
shared-config-directory declaration. Syntax and public-analyzer regressions also establish
identical production identities/content/output under ordinary roots, external `__tests__`
ancestors and repositories themselves named `__tests__`; real repository-relative test
paths remain controlled by the test toggle. The broad-suite counts in this section precede
these fixes; subsequent validation is focused, with strict all-targets Clippy and formatting.
The standalone fixed-TS configuration checks run with
`node --test src-tauri/src/structure_review/typescript_analyzer/tests/config.test.mjs`
after provisioning the fixed development tools. These 14 checks, 45 focused Rust unit
tests, and 13 serialized actual-service/public-graph tests passed. The controller then
reran the affected real TS/JS Review lifecycle fixture on the fresh test binary outside
the Cargo lock; it passed with 1 test and 698 filtered tests.

The frontend suite passed 381 tests in 43 files. Type checking and ESLint passed, and the
production build completed with the existing non-fatal warning for chunks larger than 500 kB.
Real rendering checks at 1440×900 and the supported 900×600 minimum covered TS source,
TS-to-JS calls, JSX component relationships, duplicate declaration cards, the test toggle,
separate before/after notes and popup bounds without page-level horizontal overflow.
The setup-script suite passed 166 tests without failures or skips.

The research documents remain historical experiments; the acceptance above comes from the
production adapter and application paths. Native service execution was verified on macOS.
Windows shared-LSP production code and tests were cross-type-checked, but Windows and Linux
were not executed natively. At that verification point packaged distribution of the fixed tools was deferred, and
the repository and language-service file-host checks do not provide an OS-level sandbox.

The shared-workspace revision on 2026-09-15 removes the copied analysis tree and all
source/standard-library reverse mapping. Its fixed-service regression creates a real Git
worktree, verifies original member/definition paths and byte-identical workspace contents,
and checks that the temporary directory contains only Narrant helpers/control metadata.
All 81 TS adapter tests (including opt-in services and every cleanup exit), 21 JavaScript
configuration/file-host tests, 20 application tests and the real TS/JS Review lifecycle
fixture passed. All-targets Clippy with warnings denied and focused helper ESLint passed.
The earlier full Rust/frontend acceptance remains applicable; no shared production
contract or UI behavior changed in this revision.

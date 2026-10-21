# Independent language analyzers

Status: implementing. Revises unmerged #205 after direct answers Q6–Q11.
Do not repeat completed extraction or discard regression coverage.

Spec: [0027](../specs/0027-independent-codegraph-project.md),
[ADR-0038](../adr/0038-call-independent-language-analyzers.md), and
`/Users/lizhanfa/projects/CodeGraph/docs/specs/0001-standalone-language-analyzers.md`.
The older plan retains historical evidence; its generic CLI design is superseded.

## Global Constraints

- Producer: `/Users/lizhanfa/projects/CodeGraph`, branch
  `codex/issue-205-standalone-engine`, baseline
  `e07bf26356d004763c1173771cd4a7a16e2a8ed3`.
- Consumer: `/Users/lizhanfa/.codex/worktrees/688d/narrant`, branch
  `codex/issue-205-codegraph-extraction`, baseline
  `c3307f019b9779cebb6f72a3d77900de7b44c1db`.
- Each language analyzer is directly callable. No required umbrella CLI, public
  language selector, `--pack`, `--pack-root`, runtime argv assembly or private
  engine protocol in consumers. A package may contain dependencies.
- Fixed entries: `bin/codegraph-<pack-id>[.exe]`, IDs `rust`,
  `typescript-javascript`, `python`, `java`, `go`, `csharp`, `c-cpp`.
  Each accepts `analyze`, `--help`, `--version`; compiled language identity must
  match its package. Do not infer identity from argv[0] or copy one generic binary.
- Keep unreleased `0.1.0`, public protocol `codegraph.analyzer/1` and five-field
  `control/codegraph.json`. Old candidates have no published compatibility
  obligation. Any necessary schema change needs an explicit documented ruling.
- One absolute `repository_root`, optional `files`, existing `options`.
  Omitted files means configuration discovery; [] means empty; null/malformed
  selection is invalid. Explicit files never expand through references.
- Discovery belongs in language loaders within the same analysis request and
  snapshot. Respect source roots, workspace/module membership, include/exclude
  settings and test roles. Do not select all VFS/context/dependency files or
  substitute a blind recursive extension scan for configuration.
- Preserve six graph categories, identity, content/source, containment,
  references/external symbols, test switch, Rust scopes, read-only policy,
  strict source/coverage validation and bounded process lifecycle.
  Validate all discovered paths before graph conversion.
- Keep macOS ARM64/Windows x64, one exact release for all seven analyzers,
  artifact verification and required semantic coverage. Do not weaken CI gates.
- Narrant owns install/leases/routing/merge and graph differences. CodeGraph owns
  analyzers, target SDK discovery, builds/releases. No SDK auto-install or project
  build-script execution.
- Every Cargo command uses the repository's `scripts/cargo-agent.sh`; independent
  targets and incremental remain. No parallel cold Rust builds.
- Chinese comments on methods/types; Rust unit tests in module `tests/`.
  No UI layout work planned.
- Preserve user's model settings. Fresh subagents do not inherit history or
  spawn agents. One implementer at a time, task review then whole-branch review.
  No main push, merge or release before corresponding human merge boundary.

## Task 1: Fixed entries and source organization

Work only in CodeGraph. Read producer spec and Global Constraints. Include the
already prepared glossary/spec in the commit.

Replace generic public CLI with seven separately built entries at the exact
paths above. Factor stdio supervision, graph validation and lifecycle into a
private `publish = false` runtime library. Bind a concrete language adapter in
each entry at compile time. Move language-switch/factory selection out of public
runtime into adapters; runtime receives the bound adapter. Consumer invocation
is `<entry> analyze`. Derive package root from executable location, canonicalize
and validate it, verify compiled identity against control/codegraph.json.
Private worker preserves fixed identity and parent-loss/descendant cleanup.

Move native-helpers/ to analyzers/; update active source/build/test/workflow
references without losing cases. Replace Narrant/native-helper product and
executable identities with CodeGraph analyzer/engine names, consistently updating
manifests, locks and fixtures. Historical docs may retain dated names. Private
protocol can be atomically renamed inside this unreleased producer.

Adapt build helpers to construct each entry; remove generic executable target.
Task 5 owns final archive/release matrix and complete acceptance. Keep existing
explicit selection; Task 2 adds directory discovery.

Verify fmt, script syntax and focused script path/build tests, runtime checks,
public protocol/lifecycle tests. Test actual entry rejecting wrong package and
old selectors, independence from cwd, cancellation/EOF/parent loss. Full seven
language builds wait for Task 5. Commit; report exact tests and remaining seams.

## Task 2: Discovery contract and Rust

Work only in CodeGraph. Parent supplies Task 1 adapter/runtime paths. Read spec
and Global Constraints.

Distinguish omitted files, explicit [], nonempty, invalid null/malformed paths.
Keep empty fast-path independent of SDK. Extend private request unambiguously
(prefer omitted input.files for discovery too), while explicit requests remain
strict. Document exact private representation for following tasks.

Discover within one engine request; loaders resolve selection before projection
and return complete coverage. Runtime validates discovered relative paths,
duplicates, containment, source types and origins, then normal graph invariants.
Explicit mode retains exact requested coverage. Bound discovery/output, retain
timeouts/cancellation/cleanup. Missing/invalid config cannot become empty success.

Rust discovers loaded workspace members, declared targets and actual module
graphs, including nested modules; exclude dependency crates and unrelated VFS
context. Reuse read-only Cargo/static loading and all three dependency policies,
including the existing explicit network policy for AllDependencies. Discovery
must not introduce an extra fetch/install step or broaden network permissions.
No project build scripts or target-toolchain installation; verification is offline.

Tests: omission/empty/null; path/coverage/origin failures; Rust workspace members,
nested modules and unrelated source; dependency context outside graph; test switch;
invalid config/read-only. Run focused runtime/Rust tests via cargo-agent.
Other languages may reject discovery until Tasks 3–4, but explicit mode stays
compatible. Commit; report precise internal contract and example fixtures.

## Task 3: TypeScript/JavaScript and Python discovery

Work only in CodeGraph after Task 2. Read spec/Global Constraints and parent-
supplied private discovery contract.

TS/JS uses parsed tsconfig/jsconfig fileNames, references, allowJs and include/
exclude policy; declaration context is not automatically selected source.
Python needs root-first configuration discovery instead of first-selected-file
bootstrap; honor project-owned source roots and .py/.pyi membership. Apply the
pinned ty SrcOptions include/exclude through the existing read-only in-memory
project snapshot for discovery; do not enable automatic disk/environment loading.
Keep explicit-file semantics unchanged. extra-paths/dependency/stdlib context
is not selected merely because it is loaded.

Emit complete selection coverage; preserve explicit selection/empty, external
references, test filtering, cancellation and read-only behavior.
Test real directory graph, config membership/exclusion, invalid config, explicit
empty/nonempty, selected A referencing unselected B, and no writes. Run focused
language/runtime tests, Cargo via wrapper. Commit and report discovery policies.

## Task 4: Go, Java, C# and C/C++ discovery

Work only in CodeGraph after Task 3. Read spec/Global Constraints and private
contract supplied by parent.

- Go: root go.mod/go.work and workspace members, supported build constraints,
  package membership/test role. Replace selected[:1] bootstrap for discovery.
- Java: supported POM/static metadata, declared modules/source roots/test role.
  Separate config discovery from selection matching; no project builds.
- C#: StaticConfiguration.Files, Compile Include/Remove and supported conditional
  TFM/Configuration. Preserve target reference isolation and Windows
  File.OpenRead + XDocument.Load(stream) fix.
- C/C++: compilation database translation units and repository headers actually
  participating in them; exclude external/system headers. Preserve all-config
  coverage and approved target/platform rules.

Reuse loaders within one request, no blind scan or missing-config empty fallback.
Per-language tests cover directory graph, config membership/exclusion, explicit
empty/selected-only, outside references, invalid config, read-only, changed
cancellation paths and portable Windows path fixtures.
Run focused language/runtime tests; report CI-only prerequisites honestly.
Commit and report exact commands/results.

## Task 5: Packaging and complete producer acceptance

Work only in CodeGraph after Tasks 1–4. Read spec/Global Constraints.

Finish build/archive/manifest/verification for fixed entries and bundled
dependencies. Remove generic CLI archive/binary, standalone checksum/release
expectations and both-choice tests. Each package contains its own public entry;
callers pass no selectors/private config.

Keep `<id>-<target>-pack.tar.gz` and manifest names/version. Release contains
14 archives, 14 manifests, SHA256SUMS (29 assets).
Preserve path/hash/size/executable/identity/clean-source validation, immutable
publication guards, complete CI receipts and SDK packaging policy.
Update README/protocol/layout/distribution/examples to final design; do not claim
unpublished URLs work.

Real public acceptance for all seven: direct installed entry, omitted files with
configuration selection, [] and selected files, outside symbols, tests option,
malformed/missing config, read-only and lifecycle. Keep retained semantic inventory
and platform distinctions; only genuine additions/renames change receipts.

Run scripts/fmt/affected runtime tests; sequentially build and verify candidates
using existing pinned caches/wrappers. Parent pushes draft checkpoint for automatic
macOS/Windows CI. No manual quality-gate/merge/publish.
Commit; report candidate roots/identities, checks and remaining remote validation.

## Task 6: Narrant consumes fixed analyzer entries

Work only in Narrant after Task 5 supplies candidates. Read AGENTS.md,
docs/language-support-packs.md, docs/codegraph-consumer-acceptance.md, spec0027
and Global Constraints. Include parent-prepared spec/ADR/plan docs in commit.

Change package validation, codegraph_client launch, installer/fixtures/scripts to
`bin/codegraph-<id>[.exe] analyze` with fixed public identity.
Remove --pack/--pack-root and generic binary assumptions. Narrant still explicitly
supplies files, owns install/leases/routing/merge and computes graph differences.
Do not import private descriptors, engine argv, runtime or SDK catalogs.

Keep all 38 diff cases, real graphs, install/retry/restart/shared locks,
Review DTOs, process/workspace/lease barriers and multi-language behavior.
Update active terminology; historical test names may stay if mapping remains clear.
Keep published:false until actual release. If downloader touched, fix partial
failure residue so incomplete files do not block a retry.

Verify fmt, check --lib, focused client/installer/process/Review tests, scripts
and real candidate consumption. Follow AGENTS browser rules for any UI changes.
Commit; report exact candidate identities, checks and remaining release evidence.

## Task 7: Final review, publication and consumer gate

Parent coordinates full producer revision and final consumer extraction review,
preserving original/new rulings. Resolve findings under SDD. Push draft revisions
for new exact-head dual-platform Complete CI; old e07bf26 evidence is historical.

Once implementation/review/CI complete, update CodeGraph PR title/body to final
design and mark ready. Human retains merge authority; stop at that concrete
boundary if not yet merged.

After human merge verify exact main source/Complete CI; use authorized first-
release workflow with immutable artifacts, verify actual downloaded bytes.
Then switch Narrant's seven published flags from real release evidence, run
downloads/consumption, open/attach PR and require latest Complete CI.
Wait for human review/merge.

Only after both merges, release, consumer acceptance and cleanup perform loop
closeout/complete. Do not close #205 or release worker slot early.

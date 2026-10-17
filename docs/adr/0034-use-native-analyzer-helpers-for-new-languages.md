---
status: accepted
date: 2026-10-02
---

# Use native analyzer helpers for new languages

Historical scope note (2026-10-11): [ADR-0035](0035-migrate-existing-analyzers-to-native-apis.md)
supersedes this decision's exclusion of existing Rust, TS/JS and Python analyzers. All seven
helpers now use the common Host; the LSP/Python sharing descriptions below record the
2026-10-02 design and its earlier validation, not a retained production fallback.
Migration-specific final two-platform acceptance is still pending.

Java, Go, C#, and C/C++ support will use Narrant-controlled native analyzer helper
processes behind a common process interface. Each language's helper uses its native
analysis libraries to obtain both syntax and semantic information needed to construct
a Code Graph. A separate language server and LSP connection are not mandatory stages
of this analysis path. This records the architecture choice, not a claim that any
candidate backend or support-pack version has passed acceptance tests.

Helpers return normalized Code Graph construction data rather than raw compiler ASTs
or low-level observations requiring a second language implementation in Rust. Each
helper owns language-specific object classification, content projection, stable
identity, graph membership, and reference-target classification. Rust owns the common
process adapter, input/provenance boundaries, decoding, shared validation, final
Code Graph construction, and lifecycle integration. A structurally valid graph does
not itself prove language-semantic correctness; each helper still needs its own
conformance fixtures. The wire contract in
[specification 0024](../specs/0024-native-analyzer-helper-protocol.md) is implemented;
the fixed profiles have passed the new native path's macOS ARM64 and Windows x64
helper, Host, installation and public graph tests. The
[cross-platform record](../research/native-crossplatform-validation.md) records the
passing shared WindowsJob fix at `83d5642` and subsequent verification separately.
Support packs remain unpublished and the PR remains a draft.

Using one native analysis model for declarations and symbol binding reduces the need
to reconcile an independent syntax parser with language-server results. The trade-off
is maintaining helper implementations in additional languages, a versioned process
contract, and their runtime and lifecycle integration. Representative fixtures now verify
the fixed implementation scope; long-term maintenance advantages still require experience.

Existing Rust, TypeScript/JavaScript, and Python analysis implementations remain unchanged. New
adapters continue to satisfy the public `LanguageAnalyzer` / Code Graph contract.
Migration of existing analyzers is a separate future decision, considered only after
the new implementations demonstrate a clearer and more maintainable design.

LSP/Python and native helpers share one `child_process::ProcessTree`. Windows uses
the verified Job owner and handle tracker; Unix uses an independent process group
with a member snapshot and exit barrier. Callers retain their protocol and timeout
policy. Shared lifecycle fixtures cover both call paths without duplicating platform
ownership or test coordination. Direct-child cleanup still runs after a platform
error, and the original failure remains visible. The Windows completion-ordering
regression and admission-limit counterexample remain tests of the production owner.

Helpers retain the workspace and child-process lifetime obligations of
[ADR-0031](0031-reuse-review-head-for-structure-analysis.md) and the support-pack versus
project-toolchain separation of
[ADR-0032](0032-separate-language-support-packs-from-project-toolchains.md).
All four new language implementations exclude execution of build scripts, plugins,
annotation processors, and source generators carried by the analyzed project or its
dependencies. This generalizes the earlier Java exclusion of Maven/Gradle/JDT LS
automatic build import. Controlled analysis tools may run, but project loading cannot
be used to execute the excluded logic; in particular, MSBuild design-time import is
not an approved C# path. Reading static configuration or existing permitted metadata
is distinct from running it. This policy alone does not prove filesystem read-only
behavior; the workspace contract still needs independent verification.

Initial semantic context is the fixed side's repository sources plus standard-library
or platform declarations for the explicitly supported target. Do not download,
restore, or install project dependencies, and do not expand context using the user's
installed or cached third-party project dependencies. This follows the existing
default analysis scope rather than adding the previously proposed existing-library
exception. Identification of repository sources and necessary static metadata remains
language-specific; an installation directory is not automatically repository source.
Installing a validated Language Support Pack remains distinct from installing project
dependencies. Following the 2026-10-04 refinement, C# discovers locally installed
`Microsoft.NETCore.App.Ref` packs matching each modern `netN.0` target, selecting the
highest stable patch per target; reference declarations are not bundled in the
C# support pack. macOS and Windows C/C++ discover installed system-header SDKs
without a product version whitelist. The Host records actual versions and exact
approved roots once for the comparison. Missing target declarations fail explicitly;
discovery never runs project commands or installs SDKs. A fixed private helper runtime
and compiler remain separate from these project targets. Java likewise discovers
installed JDKs, choosing the highest stable patch per major and matching each
configuration's release; the private runtime is a candidate only for its own major.
Go discovers installed SDK source trees and chooses the highest stable patch in the
effective declared minor, never substituting a newer minor. Neither pack bundles an
additional project-target SDK. Capability comes from the actual JDT / Go compiler,
not a product target-version allowlist. C/C++ takes each TU's explicit language mode
and standard from the compilation database, using LLVM's native standard table;
missing configuration and preview modes fail explicitly. See the
[target-version record](../research/native-target-version-discovery.md) for selection
rules and representative acceptance results.

Rust's existing opt-in all-dependencies mode remains unchanged; it is not the default
for the new implementations. Missing third-party context cannot be used to invent
resolved targets, and analyzer failures cannot become successful empty graphs.

The initial implementation retains the complete-graph contract: when supported
objects are fully extracted, uncertain relationships may remain Unresolved; if object
extraction cannot be completed for the chosen scope, analysis fails explicitly.
Do not add a partial-structure-result mode in this version. Configuration discovery
and language-specific branch coverage follow the provisional policy below; this
failure policy does not implicitly approve a single-configuration scope.

Each new analyzer returns one Configuration Union Code Graph: an object or resolved
relationship is present if it is established in at least one analyzed configuration.
The output does not retain per-fact configuration attribution or expose separate
configuration views. This is a union of evidenced facts, not a claim that mutually
exclusive declarations can be analyzed as one program, that every fact applies to
every configuration, or that every path through the union is executable in one
configuration. Static call edges do not imply inevitable or simultaneous execution.

The helper performs any necessary configuration-specific analysis and compatible
source-object aggregation before returning the common construction data. It must not
overwrite conflicting content, invent targets, or use a configuration-specific node
copy to hide an unresolved source-identity problem. An object or containment conflict
that cannot be represented correctly under the common contract is an explicit failure,
not a partial graph. This choice supersedes the proposed per-configuration result
container, conditional-graph schema, and variant UI as the default product direction.

The user has provisionally chosen automatic discovery of explicit, statically
determinable configurations for the helper's internal work. The discovery mechanism
and language-specific coverage remain exploration topics; the accepted union output
is not a promise to enumerate every possible macro or build-flag combination. Internal
configuration provenance may support validation and cache correctness without becoming
configuration attribution in the public Code Graph.

The [common protocol](../specs/0024-native-analyzer-helper-protocol.md) and
[language profiles](../specs/0024a-native-language-analysis-profiles.md) define
the implementation contract. Dynamic target discovery is bounded by compiler capability
and required declarations; unsupported target shapes fail explicitly. The recorded
matrix supplies representative validation, not a product version allowlist or a claim
that every SDK and project shape has passed. The
[interface design discussion](../research/native-analyzer-contract-design.md)
retains the earlier decision history; it is not evidence for untested configurations.

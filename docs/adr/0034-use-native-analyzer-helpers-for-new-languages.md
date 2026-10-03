---
status: accepted
date: 2026-10-02
---

# Use native analyzer helpers for new languages

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
[cross-platform record](../research/native-crossplatform-validation.md) separates
these results from the unchanged legacy WindowsJob failure that still makes the
overall CI run fail. Support packs remain unpublished and the PR remains a draft.

Using one native analysis model for declarations and symbol binding reduces the need
to reconcile an independent syntax parser with language-server results. The trade-off
is maintaining helper implementations in additional languages, a versioned process
contract, and their runtime and lifecycle integration. Representative fixtures now verify
the fixed implementation scope; long-term maintenance advantages still require experience.

Existing Rust, TypeScript/JavaScript, and Python analyzers remain unchanged. New
adapters continue to satisfy the public `LanguageAnalyzer` / Code Graph contract.
Migration of existing analyzers is a separate future decision, considered only after
the new implementations demonstrate a clearer and more maintainable design.

The new helpers use a dedicated Windows process owner with one handle tracker.
Limited suspended-startup primitives are duplicated to avoid changing existing
LSP/Python behavior; the legacy owner's known completion-ordering defect remains
separately recorded and its failing test is retained.

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
dependencies. Target SDK provisioning is not settled by this decision.

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
the implementation contract. Project configurations, target SDK profiles and backend
versions beyond the recorded matrix still require the stated validation gates. The
[interface design discussion](../research/native-analyzer-contract-design.md)
retains the earlier decision history; it is not evidence for untested configurations.

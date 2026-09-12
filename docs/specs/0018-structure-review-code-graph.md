---
status: proposed
---

# Structure Review Code Graph

## Purpose and scope

Build complete code-object graphs for the same caller-selected file scope before and
after a Change Comparison, then compare those graphs. Place the capability under
`src-tauri/src/structure_review/code_graph/`.

The latest proposal uses one language-neutral virtual root, a graph-owned node table,
six CodeNode enum variants, child IDs, language on each node, and outgoing references.
Language implementations supply the
language-native declaration signatures; the common module validates and compares
the resulting data. The Rust adapter is implemented separately under
`structure_review::rust_analyzer`; frontend rendering remains later work.

- Extract all supported high-level objects in the selected files before comparison.
  Do not preselect affected functions from diff hunks.
- Keep the six common categories: Container, Type, Callable, Value, ExecutionRegion,
  and Extension. Preserve each object's language and native kind.
- Express containment through a single child-ID list. Store non-containment references on
  the source node, with an explicit semantic and an inline target.
- Supply a normalized declaration signature rather than decomposing parameters,
  receivers, generic constraints, visibility, or value types in the common model.
- Store language-normalized own code to support body-only comparison, following the
  user's Rust-analyzer design decision, in the optional `CodeNodeData.code` field.
- Defer source locations, a separate attributes object, a graph-wide symbol table,
  and extension-field version envelopes.
- Keep identity matching conservative. Do not infer object renames or moves.
- Consumers decide display, filtering, layout, and ancestor-impact presentation.

The earlier flat relations/symbol-table layout and detailed signature payloads are
superseded by this proposal. See [Code Object Model](0018a-code-object-kinds.md),
[Code Relationships](0018b-code-relationships.md), and the concrete
[Rust Interface](0018c-rust-interface.md).

## Current implementation

The common Rust interface, graph construction validation, node lookup, and graph
difference calculation are implemented in
[structure_review::code_graph](../../src-tauri/src/structure_review/code_graph/mod.rs).
Tests use the public interface and live in the module's tests/ directory.

LanguageAnalyzer and AnalysisInput remain the language-implementation contract. The Rust
adapter implements it through rust-analyzer LSP and a local syntax library. Preparing
before/after repository directories and frontend transport remain caller responsibilities.
The common comparison includes normalized own code and can detect body-only changes.

The accepted [Rust Language Analyzer](0019-rust-language-analyzer.md) specification
records the adapter scope and the real-server verification obligations.

## Language interface and input

`LanguageAnalyzer::analyze(&AnalysisInput)` returns a CodeGraph or an analysis error.
AnalysisInput supplies a repository_root and a list of repository-relative file paths.
The caller prepares the before/after repository states for the same intended scope
and uses the same language implementations and configuration on both sides. Each
directory and the analyzer's view of it remain consistent for the duration of analysis.

files selects which files contribute objects to the output graph. It does not prevent
an analyzer from reading other source, configuration, or dependencies for semantic
context. A resolved target outside the selected graph remains an External reference.
The input paths are relative, stay within the root, and identify existing source files
on that side. A new file appears only in the after input; a deleted file only in the
before input. Empty files are included when selected. Failure to read a requested
file is an error rather than a successful empty result.

This input supports LSP implementations while keeping the common interface independent
of the protocol. LSP initialize supplies workspace folders, and document operations
identify files by URI. Open documents are synchronized through client-provided content
and versions, so the implementation must not mix the requested repository state with
stale or unrelated editor buffers.
[LSP Initialize](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#initialize),
[LSP Document Synchronization](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_didOpen).

Project context can matter beyond the selected files: rust-analyzer recognizes Cargo
projects, rust-project.json project descriptions, and standalone files, with different
available dependency and build information.
[rust-analyzer Configuration](https://rust-analyzer.github.io/book/configuration.html),
[Non-Cargo Projects](https://rust-analyzer.github.io/book/non_cargo_based_projects.html).

LSP document symbols, call hierarchy, and type hierarchy are optional server
capabilities. They do not guarantee direct coverage of this model's six categories
or every relationship. Mapping and any additional extraction remain each language
implementation's responsibility; unavailable analysis must not masquerade as a known
absence of declarations or relationships.
[LSP Document Symbols](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_documentSymbol),
[LSP Call Hierarchy](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_prepareCallHierarchy),
[LSP Type Hierarchy](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_prepareTypeHierarchy).

A CodeGraph can contain objects from several languages. Each language implementation
can analyze its own source scope, with composition handled by the caller or a composite
analyzer. The same language implementation handles each language on both sides; a
newly added source language is not by itself an incompatibility.

Language belongs on CodeNode. The virtual root has no source language, and the graph
does not claim that every descendant shares one language. References may connect
different node languages when that association is known. Automatically resolving
cross-language bridges or genuinely embedded languages is later analyzer work, not a
required first-version capability of the common graph.

TSX is handled as TypeScript with JSX syntax in the initial mapping. JSX is XML-like
syntax, not a separate HTML source file; it can be transformed to JavaScript or
preserved for another transform. No separate HTML graph is required by that fact.
[TypeScript JSX](https://www.typescriptlang.org/docs/handbook/jsx.html#basic-usage).

Cross-language invocation is real: Tauri exposes Rust commands to frontend invoke
calls. The graph can represent a resolved bridge with a suitably defined reference
semantic without claiming it is an ordinary direct language-level function call.
[Tauri Calling Rust from the Frontend](https://v2.tauri.app/develop/calling-rust/).

CodeGraph owns each node once and exposes read-only root/node access. Construction
accepts the root and extracted nodes, builds the index, and validates containment and
references; no CodeGraphParts wrapper is required. Language analyzers own project
reads and may use LSP; the common graph/diff code does neither. Revision selection
and preparation of repository states remain with the caller. Retry policy,
scheduling, caching, and persistence remain outside this data contract.

NodeId and External symbol identities must not include the absolute checkout prefix.
The same semantic object needs a corresponding identity when analyzed in two different
directories. LSP document URIs are not by themselves stable cross-checkout identities.

## Graph structure

CodeGraph has one virtual root and one private node table. root().contains holds only
top-level node IDs in a single list. Each CodeNode variant carries shared CodeNodeData,
which records its `contains` child IDs and outgoing `references`.

The CodeNode enum supplies the common category. Type covers classes, structs,
interfaces, traits, enums, and aliases; native_kind retains their differences.
No parent-side category lists need to be kept consistent with the child node.

Each node identity occurs once in the declaration hierarchy in this proposed shape.
Other ownership, reuse, or target associations use references. This is a single
declaration placement convention enforced by validation. ID-based storage could
represent multiple structural parents, but changing that convention requires a
deliberate contract decision; it never requires duplicating node values.

The earlier public CodeGraph.nodes field represented top-level nodes. The current
private nodes field instead owns all node values. CodeGraph.root() continues to be
the navigation entry point, so this storage change does not flatten the hierarchy.
The virtual root is not a source-code Container: it has no native kind, signature,
language, or node ID. Following containment IDs from it reaches every graph-local node.

Callers can group a module's children by matching their CodeNode variants. Such a
grouped view is derived from the nodes, not another authoritative category store.
Both containment and other references target IDs rather than recursively owning values.
CodeGraph.node(id) returns a borrowed node from the sole node store; no second owning
tree or caller-maintained index is needed. A separate global edges array is optional.
Node identities must be unique across the entire graph, including language scopes.

Source files remain caller input, not automatic semantic containers. This revision
defers source navigation and multiple declaration locations instead of introducing a
partial or fabricated source-location mapping.

## Own-content comparison

Each node has a stable identity, language, native kind, optional name, optional signature,
optional normalized own code, and a simple map of semantic extensions. Its common
category comes from its enum variant. The derived CodeCategory is used for querying; it is
not a second independently editable category on the input node.

The signature is readable declaration code normalized by the language implementation.
It includes relevant declaration modifiers and preserves language-specific syntax.
A class declaration excludes its members; a callable declaration excludes its body.
A value's declaration can be represented as a signature without decomposing its type
or initializer into common fields.

Pure comment/formatting changes must not create semantic changes. Do not implement
normalization by blindly deleting whitespace or comment-looking text inside literals.

Child membership and outgoing references are compared separately from the parent's
own declaration. Recursively comparing the serialized subtree as a parent's own
content would incorrectly mark that parent modified for every descendant change.

## Body comparison decision

During the Rust analyzer design, the user chose to store code instead of an opaque
content fingerprint. This supersedes the earlier exclusion of public body source.
Body-only change detection remains part of the intended capability.

Ordinary function implementation content belongs to its Callable, without requiring
an ExecutionRegion wrapper. Comparison still distinguishes a node's own content from
the content of separately modeled children, so a child edit does not automatically
modify every ancestor's own declaration.

The user selected language-normalized own code in the
[Rust analyzer design](0019-rust-language-analyzer.md). It preserves significant syntax
and literal contents while ignoring pure formatting/comment changes, consistent with
the existing signature convention. No opaque comparison fingerprint is required.
The implemented `CodeNodeData.code` participates in own-content comparison. `None` means
that the object has no implementation content, not that extraction failed. A callable
with an empty body still has comparable code.

For example, changing a callable body from returning one literal to another while
leaving its signature and references unchanged cannot be detected from signatures and
references alone. Do not claim otherwise or silently remove the original capability.

## Difference result

The proposed result groups changes by node identity. NodeDiff.content is an
Option<ChangeKind> containing Added, Removed, or Modified for the node's own data.
None indicates unchanged own data, even when direct-child membership or outgoing
references changed. NodeDeclaration snapshots are omitted: callers already retain
both graphs and can retrieve full node values by ID. The module computes the status;
callers need not repeat the comparison merely to find modified objects.

Direct-child and reference changes still list the added/removed elements explicitly.
A parent record with only a child/reference change does not mean the parent's own
declaration is modified. A node with no change in any of these dimensions is omitted.

The virtual root remains an entry point in each original graph, without its own diff
record. Consumers needing its entry membership can read or compare both root lists.
Node additions/removals alone do not determine root membership when a retained node
changes declaration placement. The difference is used with the two input graphs;
it is not a standalone patch for reconstructing the after graph.

- Added and removed IDs create additions/removals.
- A retained ID with changed own declaration data creates a modification.
- Containment and reference changes are independently observable.
- Changed reference target/semantic is removal/addition.
- Unchanged own data produces declaration: None, even in a relationship-change record.
- The caller retains both input graphs to find unchanged context and targets.

Analysis capabilities and normalization must remain fixed across the comparison.
Unsupported analysis or a failed extraction cannot be substituted with a successful
empty graph. Explicit unresolved references remain distinguishable from identified
targets; differences in such observations are not proof of changed runtime behavior.

## Repository integration and verification

Use fixed Comparison Base and Head revisions, consistent with
[ADR-0010](../adr/0010-use-current-three-dot-comparisons.md). Keep revision resolution
and preparation of their repository directories with the caller, preserving the
source-selection separation in
[ADR-0018](../adr/0018-put-source-resolution-outside-local-change-content.md).
Expose the public interface through the capability's mod.rs, consistent with
[ADR-0019](../adr/0019-organize-rust-backend-by-capability.md).

The existing SnapshotContentReader supplies full pinned contents for changed files,
not a complete project directory suitable for workspace analysis. Repository-state
preparation must supply the project context needed by the selected language analyzer.
File selection still distinguishes actual source from binary files, symlink targets,
submodule IDs, and absent sides.

Common-module contract checks cover all six node variants, nested declarations, node-local
references, signature changes, child-only changes without synthetic parent edits,
globally duplicate IDs, invalid local reference targets, containment cycles, multiple
declaration placements, unreachable stored nodes, mixed-language roots and
references, deriving root entry membership from both retained graphs, deep containment,
deterministic difference ordering, and unordered or repeated reference observations.

Concrete language/caller implementations must verify repository-relative path
validation, missing requested-file errors, per-side selection for added/removed files,
and stable identities across different checkout roots. The code-content extension
includes focused tests for body-only modifications and independent reference changes.

Document Rust types and methods in Chinese. Keep Rust tests in module tests/
directories; all Cargo validation uses scripts/cargo-agent.sh. Concrete implementation
and test locations are listed in the [Rust Interface](0018c-rust-interface.md).

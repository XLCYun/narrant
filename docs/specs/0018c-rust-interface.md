---
status: accepted
---

# Code Graph Rust Interface

This revision uses a CodeNode enum for the six common categories, replacing the
earlier six CodeNodes buckets. It retains one explicit root, child-ID containment,
node-local references, declaration signatures, and language on each node. One
graph-owned node table stores each node exactly once. The root remains the navigation
entry point; a rooted hierarchy does not require recursively owning node values.

The common content/tree/reference interface is implemented, including normalized
`CodeNodeData.code` for body-only change detection. See the
[body comparison decision](0018-structure-review-code-graph.md#body-comparison-decision).
The concrete Rust LSP adapter lives in the sibling `rust_analyzer` module; frontend
transport remains later work.

`CodeNodeData.source` optionally retains the node's original source for display,
including declaration, implementation, comments, whitespace and nested members.
It is separate from normalized `signature` and own `code`: neither node identity nor
`CodeGraph::diff` compares it. An analyzer can leave it `None` when no corresponding
source is available. No filesystem location or formatter is required by this field.

## Rust implementation

The supported entry point is
[structure_review::code_graph](../../src-tauri/src/structure_review/code_graph/mod.rs).
Implementation files stay private; the entry point re-exports the contract types.
The Rust source is authoritative rather than a second copy of the definitions here.

| Responsibility | Source |
| --- | --- |
| Graph ownership, root access, construction and comparison methods | [graph.rs](../../src-tauri/src/structure_review/code_graph/graph.rs) |
| Six node variants, shared data, references, identities and extension fields | [node.rs](../../src-tauri/src/structure_review/code_graph/node.rs) |
| Repository/file input and language implementation trait | [analyzer.rs](../../src-tauri/src/structure_review/code_graph/analyzer.rs) |
| Difference result types and set comparison | [diff.rs](../../src-tauri/src/structure_review/code_graph/diff.rs) |
| Endpoint, declaration placement and reachability validation | [validation.rs](../../src-tauri/src/structure_review/code_graph/validation.rs) |
| Analysis, graph construction and comparison errors | [error.rs](../../src-tauri/src/structure_review/code_graph/error.rs) |
| Public-interface contract tests | [tests/](../../src-tauri/src/structure_review/code_graph/tests/mod.rs) |

CodeGraph::new builds the sole node index and rejects invalid graphs. Single declaration
placement plus reachability from the root rules out containment cycles, including
cycles disconnected from the root. Traversal uses an explicit stack. Reference cycles
and repeated identical reference observations are allowed.

`CodeGraph::with_external_symbols` optionally attaches a map from opaque External
symbol identity to `ExternalSymbol { name, scope }`. `external_symbols()` exposes
these descriptions read-only. `SymbolScope::{Repository, Dependency}` distinguishes
repository symbols outside the selected graph from source outside the repository.
Names and scope are analyzer-provided descriptions, not additional nodes or fields
in reference identity; changing them alone does not create a graph difference.

CodeGraph::diff produces stable output: nodes are sorted by NodeId, and set changes
list sorted removals followed by sorted additions. It ignores input array order and
duplicate reference observations. Own data is compared separately from relationships,
without generic source-text normalization or rename/move inference.

All three error types implement Display and std::error::Error. The current comparison
of validated in-memory graphs has no additional fallible operation and returns Ok;
the Result signature is preserved. Matching analysis and normalization conventions is
a caller precondition, not something inferred from absent protocol-version metadata.

`AnalysisError::new(message)` represents a real failure;
`AnalysisError::cancelled(message)` explicitly represents cooperative cancellation.
`is_cancelled()` preserves that distinction independently of a signal arriving later,
so simultaneous user cancellation cannot hide a real analyzer failure. The message
remains available for diagnostics, and language implementations do not need access
to the parent's cancellation authority.

AnalysisInput, AnalysisOptions and LanguageAnalyzer define the language-implementation seam.
`AnalysisOptions::default()` sets `include_tests` to false; the option is carried in
`AnalysisInput.options` and must match on both sides of a comparison. The language
implementation owns conservative test classification; the common graph does not
interpret Rust attributes or any other language's testing conventions. Path checks,
source reads and normalization belong to the concrete analyzer; workspace preparation
remains with the caller. RustLanguageAnalyzer implements this seam in its own module.
Serialization and deserialization are not added by this implementation; any future
wire adapter must preserve graph construction validation.

## Why an enum replaces category buckets

The earlier CodeNodes grouped children directly into six lists. That supported
queries such as module.contains.types, but after switching to centrally stored nodes
the lists contained only NodeIds. They did not enforce the target's category through
the Rust type system. A node looked up through a reference also needed its category
without inspecting the parent's collection.

CodeNode now owns its common category through its enum variant. contains expresses
only parent/child membership, and consumers can group the resolved children by
matching variants. There is no independently mutable category on the input payload,
and no parent-side category list to synchronize. CodeCategory is a derived query
value; it is not stored independently on an input node.

Every variant initially carries CodeNodeData because the current proposal keeps
the same common fields and textual signatures for all six categories. Six identical
payload structs would add duplication. A variant may receive its own payload when
a concrete category-specific requirement is agreed; this does not justify adding
unrelated structured parameters or generic constraints prematurely.

A struct with one category enum field could express today's identical payloads too.
The sum type is useful here because it makes the six alternatives explicit and can
later attach category-specific data to only the relevant variant. It does not make
child-ID existence or relationship semantics compile-time guarantees: those remain
graph-construction checks.

## What containment means

CodeGraph.root() is the single navigation entry point. Its contains holds only the
top-level node IDs. It is a virtual root, not a source module: it has no language,
native kind, signature, or NodeId. Following containment IDs through node(id) reaches
all graph-local nodes.

Each real node has one declaration placement and each NodeId appears once across
all languages. Cross-ownership or reuse is a reference to an ID, not another embedded
copy. The private node table is the authoritative storage, not a cloned cache of a
second recursively owned tree. CodeGraph.node(id) follows an ID without requiring
the caller to maintain an index. The enum variant supplies the common category;
language and native_kind retain the native distinction.

The old CodeGraphParts bundled revision/file records, objects, relationships, symbol
references, and analysis metadata. That wrapper is not restored. Construction accepts
the root and extracted node values directly; the graph builds its own storage/index.

CodeNode::Extension identifies entities in the Extension category. extensions holds
semantic attributes attached to any category. These serve different roles.

## Calling sequence

```rust,ignore
let before = analyzer.analyze(&AnalysisInput {
    repository_root: before_repository_root,
    files: before_files,
})?;
let after = analyzer.analyze(&AnalysisInput {
    repository_root: after_repository_root,
    files: after_files,
})?;
let diff = before.diff(&after)?;
```

The caller retains the fixed revisions, selected source scope, and both original
graphs. The same per-language implementations and normalization conventions produce
both sides; a composite analyzer or caller may combine language results under the
same root. No persisted-schema versions or CodeGraphParts wrapper are required.

repository_root refers to a prepared directory for one fixed side of the comparison.
The directory and the analyzer's view of its files must remain consistent throughout
analysis. Preparing those repository states stays with the caller; a language analyzer
does not select revisions or switch the user's checkout. The common graph and diff
logic do not read the filesystem or speak LSP.

files contains relative paths within repository_root, defining the object-extraction
scope. An analyzer may read other project files, configuration, and dependencies to
resolve symbols without adding all those objects to the graph. Paths must not escape
the repository root. Only files present on the current side are supplied: additions
appear only in after_files, removals only in before_files, and empty files are still
included when selected. A requested path that cannot be read is an analysis error,
not evidence of a successfully analyzed empty file.

Different checkout directories must still produce the same NodeId for the same
semantic object. Neither node IDs nor external symbol identities may depend on the
absolute checkout prefix. Converting LSP results to stable identities is the language
implementation's responsibility.

CodeGraph's storage and root are private. Construction validates globally unique node
IDs, existing local targets, one declaration placement per node, acyclic containment,
and reachability of every stored node from the root. Read-only access prevents callers
from breaking those invariants after construction. Deserialization must use the same
validation, rather than directly restoring unchecked internal fields.

Containment and references stored on nodes form ID-based adjacency lists. A separate
global edges array is optional and not part of this proposal. The full node table is
provided during construction, but it is not the mandatory navigation surface.

Wire transport remains a separate representation concern: a frontend cannot reuse a
Rust process reference as a cross-process link. A tree serialization can expand child
IDs from the node table without creating another authoritative store; a table-based
serialization can retain the same root entry point. This revision does not silently
choose a new transport contract.

TSX is initially analyzed as TypeScript with JSX syntax. JSX does not require an
independent HTML graph. References may connect nodes with different languages when
an analyzer actually resolves the association, but cross-language bridge resolution
and genuinely embedded-language analysis are not required of the first implementation.
See the [language interface discussion](0018-structure-review-code-graph.md#language-interface-and-input).

## Difference meaning

NodeDiff.content reports only the kind of own-data change, without copies of the
before/after declaration. Added and Removed indicate node existence changes; Modified
means a retained ID has different own data. None means the own data is unchanged,
even when the node has changed child membership or outgoing references. A method
changing does not recursively modify its type's declaration. Records with no own,
child, or reference changes are omitted.

The caller uses the retained before/after graphs and NodeId to obtain full node data.
The shared module computes the own-change status, so consumers need not repeat that
comparison to identify modified nodes. Field-level declaration differences are not
part of this proposal. Returning duplicate declarations without those differences
would not add that capability.

For added or removed nodes, child and reference membership are compared separately,
treating the absent node's sets as empty under successful object extraction.

The virtual root remains in each CodeGraph as a traversal entry, but it has no
declaration or separate CodeGraphDiff record. The old root-difference field reported
only membership changes among top-level entry IDs. That navigational information can
be read or compared through before.root().contains and after.root().contains when
needed. It must not be inferred solely from node additions/removals, because a retained
ID can change declaration placement. This diff is a summary used with both input
graphs, not a standalone patch that reconstructs the entire after graph.

The shared module matches node-table entries by stable IDs and compares each child-ID
list by identity/membership, not by incidental array order. Changing the category of
a retained node modifies that node's own declaration, not its parent's containment
edge. It compares normalized signatures, implementation code and semantic extensions
for own changes. It does not claim to infer whole-program semantic equivalence.

## Bodies and execution regions

For an ordinary function, body comparison content should belong to its Callable.
Creating a mandatory child ExecutionRegion for every callable would shift the direct
body edit to a lower-level node, leaving the callable's own declaration unchanged.
That is a different granularity choice, not a necessary way to represent executable
content.

Reserve ExecutionRegion for independently modeled execution outside an explicit
callable, such as module-level statements or a class static block. A distinct region
inside a callable can be considered only if independently analyzing that region later
becomes a requirement.

The user has now selected storing language-normalized own code instead of an opaque
fingerprint, superseding the initial exclusion of public body source. Function
implementation content remains owned by the Callable. The
[Rust analyzer design](0019-rust-language-analyzer.md) records this decision and the
remaining implementation boundaries.

`CodeNodeData.code` is optional normalized implementation content. `NodeDiff.content`
(formerly `declaration`) reports changes to either declarations or implementation code.
The returned status remains Added/Removed/Modified; it does not introduce field-level
diffs or duplicate before/after snapshots. The production source remains authoritative.

## Removed from the first version

- Graph-wide relations and symbol-reference tables.
- Six categorized child lists and category-bearing ChildLink records.
- Caller-supplied source text records; analysis now receives repository/file paths.
- Duplicate NodeDeclaration snapshots and virtual-root membership deltas in the diff.
- A single graph-level language, source locations, and a shared attributes structure.
- Per-category typed signature decomposition and a separate Receiver model.
- Container own_code and initialization-phase enums.
- VersionedFields and extension schema-version envelopes.

These removals do not remove native signature information or semantic reference
labels. See [Code Object Model](0018a-code-object-kinds.md) and
[Code Relationships](0018b-code-relationships.md) for the remaining conventions.

---
status: proposed
---

# Code Object Model

## Six node variants

Keep the user's six common categories as CodeNode enum variants:

| Category | Rust variant | Examples |
| --- | --- | --- |
| Container | CodeNode::Container | Packages, modules, namespaces |
| Type | CodeNode::Type | Classes, structs, interfaces, traits, enums, aliases |
| Callable | CodeNode::Callable | Functions, methods, constructors, accessors, closures |
| Value | CodeNode::Value | Fields, properties, constants, global bindings, enum members |
| ExecutionRegion | CodeNode::ExecutionRegion | Separately represented non-callable execution regions |
| Extension | CodeNode::Extension | Native constructs without a suitable category above |

Language is defined on each CodeNode, allowing different languages in one graph.
Nodes preserve a native kind such as struct, interface, getter, or impl.
The node's enum variant supplies its common category, independent of its parent or
the reference through which it was reached. CodeCategory is a derived query
value, not an independently mutable field on the input node.

Both the virtual root and node children use a single Vec<NodeId>. Each node value
lives once in the graph-owned node table. A module's types and functions are obtained
by looking up its child IDs and matching the variants. Nested types, closures, and
other declarations use the same adjacency structure.

The earlier CodeNodes buckets were intended to expose grouped children directly.
Once those buckets hold only NodeIds, they do not make category-specific children
type-safe, and a looked-up node should not depend on its incoming link for its kind.
The enum therefore replaces the six buckets. A grouped view can still be derived.

## Shared node content

Each CodeNode variant initially carries the same CodeNodeData:

- stable id;
- language, native kind, and optional declared name;
- optional normalized declaration signature and own implementation code;
- directly contained node IDs;
- outgoing references;
- a simple map of language-semantic extensions.

Source locations are deferred. The common payload now includes `code: Option<String>`
for normalized own implementation content, following the accepted Rust analyzer design.
A single node can still stand for a semantic module assembled from selected files;
the first version simply does not supply navigation to its multiple source locations.

The shared payload is justified because this version retains declaration text rather
than different structured fields for every category. Six identical payload structs
would add duplication. If a concrete requirement needs category-specific data, that
variant can receive a dedicated payload without adding irrelevant fields to all nodes.

## Signatures instead of detailed language fields

A signature is readable, normalized declaration code in the node's language. It is
not a common-language type AST. Relevant modifiers, type parameters, receiver syntax,
return declarations, and constraints remain in that signature.

Examples can include a callable header such as `pub fn load(id: u64) -> Result<Item>`,
a type header such as `struct Store<T>`, a type-alias declaration, or a value
declaration such as `const LIMIT: usize`. These are illustrative declaration forms;
language implementations define a stable normalized rendering.

No separate DeclarationAttributes, Receiver, Parameter, ReturnSignature,
GenericParameter, Constraint, TypeAnnotation, or TypeExpression is required in the
first common interface. Removing those fields does not assert that the corresponding
language concepts are absent. It avoids requiring every consumer to understand them.

The language-neutral virtual root only groups top-level declarations; it is not a
Container node pretending to be a source package. A TypeScript subtree and a Rust
subtree may coexist below that root. A TSX source declaration is initially treated as
TypeScript with JSX syntax; ordinary JSX elements do not require an HTML object graph.

Pure formatting/comments are excluded by language-aware normalization, while
significant syntax, literals, modifiers, and declaration annotations are preserved.
A signature never embeds separately represented children or a callable's body.

## Containers and execution

Container has organizational responsibilities in this model; it has no implementation
body (`code: None` in the Rust mapping). Actual module-level executable statements, if represented as graph objects,
can belong to an ExecutionRegion under that Container. An explicit constructor or
initialization method remains Callable.

This is a modeling choice, not a claim that module source cannot execute statements:
Python modules can include executable initialization statements.
[Python More on Modules](https://docs.python.org/3/tutorial/modules.html#more-on-modules).

Class static blocks may be ExecutionRegion children of a Type. Closures remain
Callable nodes. A reference from inside a nested closure belongs to that closure,
not automatically to the module or value that creates it.

No execution-phase taxonomy is required. No synthetic method declaration or semantic
module is invented merely because source text needs an organizational parent.

## Extension fields

Every category uses the same simple semantic extension map:

```rust
/// 按命名空间保存语言特有语义；第一版不附加版本封套。
pub type ExtensionFields = std::collections::BTreeMap<String, serde_json::Value>;
```

For example, an ordinary Type node can have:

```json
{
  "rust.layout": { "repr": ["C"], "align": 16 }
}
```

It remains a Type. Extension-category nodes use exactly the same map, without a second
payload. CodeNode::Extension classifies an entity; extensions holds attribute data
that can be attached to any category.

No VersionedFields or schema_version is needed for graphs produced and compared with
the same implementation and conventions in one analysis workflow. Persisted or
independently versioned graph exchange is outside the first version's scope.

Only semantic fields belong here. Common fields are not duplicated; timestamps,
diagnostics, display-only values, and source coordinates do not enter comparison.
Object-key order is irrelevant. Ordered values remain ordered unless a language
convention explicitly normalizes a semantic set.

## Comparison limit

Declaration and relationship comparison alone cannot detect a body-only change that
does not alter those facts. The user selected storing language-normalized own code
instead of an opaque fingerprint; it is now included in the common comparison.
See the [body comparison decision](0018-structure-review-code-graph.md#body-comparison-decision).

The current shape supersedes the earlier structured-payload proposal. See the
[Rust Interface](0018c-rust-interface.md) for the compact definitions and
[Code Relationships](0018b-code-relationships.md) for nested ownership and references.

---
status: proposed
---

# Code Relationships

## Containment through node IDs

CodeGraph owns all CodeNode values in one private table and has a virtual root holding
only top-level node IDs. Each node records its children's IDs in a single list; the
child's CodeNode variant determines its category. These lists express Contains; there is
no second global list of containment edges to keep synchronized.

Every graph-local node is reachable from that root through containment. The virtual
root is an entry point, not a language-defined module or another changed code object.

A module's contains list can identify a Type node representing a class, and that
class's contains list identifies its methods and fields through Callable and Value
nodes. A callable can contain another callable. Source files are not parents merely
because they contain source text. Grouping children by category is a derived query.

The proposed containment contract gives each node identity one declaration placement.
Construction validates that this is acyclic and reaches every stored node. References
express semantic membership or reuse elsewhere. ID-based storage can also represent
multiple parents, but the current tree convention would have to change explicitly;
the underlying storage never needs duplicate copies of the node.

## References on their source node

Each node owns a references collection. The source is implicit, so each reference
only needs its semantic and target:

```rust
/// 当前节点发出的非包含关联。
pub struct CodeReference {
    pub semantic: String,
    pub target: ReferenceTarget,
}

/// 区分已解析目标的位置与目标尚未解析的情况。
pub enum ReferenceTarget {
    /// 已解析，且目标节点保存在当前图中。
    Node(NodeId),
    /// 已解析，但目标不在当前图中；symbol 是带语言命名空间的稳定符号身份。
    External { symbol: String },
    /// 尚不能确定目标身份；name 只保留观测到的引用名称。
    Unresolved { name: String },
}
```

The external symbol table and ReferenceId indirection remain removed. Graph-local targets
use a node ID; out-of-scope or unresolved targets are stored inline. References never
embed another full copy of the target node, so calls and other associations can form
cycles without turning the declaration tree into recursive object copies.

External means resolved but outside this graph, not necessarily outside the repository.
For example, when extracting only page.ts, a resolved call to a declaration in an
unselected utils.ts can be External, just like a resolved dependency or standard-library
symbol. symbol records a stable, language-qualified identity rather than a bare name
or an absolute path tied to one checkout.

Unresolved means the implementation observed a reference but could not establish its
target identity. A dynamic call, unavailable resolution information, or ambiguity can
produce this state; it does not establish that the target is outside the graph or that
the source is invalid. A missing resolution result cannot simply be labeled External.

The private node table is the authoritative storage and lookup index. A separate
global edges array is not required: each node's contains and references provide its
outgoing adjacency. Callers enter through root() and follow IDs with node(id), rather
than maintaining an index of their own.

CodeGraph.node(id) lets a caller follow a node-ID reference. Containment traversal
has tree structure; traversal that also follows references must account for cycles.
An identified target outside the root's containment closure is External, not a dangling
Node reference. Unresolved targets have unknown membership. NodeId values are globally
unique across language subtrees.

References are allowed between languages. A language/framework implementation can
report a bridge such as a Tauri frontend invocation of a Rust command with a documented
semantic, for example tauri.invokes. The core does not infer such a bridge merely from
similar names, and the first version does not require cross-language resolution.

## IDs, references, and ownership

Using an ID does not mathematically require a map. A recursively owned tree could
be searched on every lookup, or indexed during a particular operation. With frequent
cross-node navigation and two-graph identity matching, a single owned node table is
the simpler current recommendation. It is a storage choice, not a reason to remove
the root-based navigation contract.

Borrowed references or shared pointers are possible Rust graph representations.
They bring different lifetime, address-stability, and ownership requirements. An
external arena can own nodes while a graph borrows them; that is not the same interface
as returning a freely owned graph from analyze. A general self-referential owned
container needs an appropriate stability design rather than ordinary unchecked
references into movable storage.
[Rust References and Borrowing](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html),
[Rust Self-referential Structures](https://doc.rust-lang.org/std/pin/index.html#a-self-referential-struct).

Rc ownership also does not eliminate the need to consider cycles; strong reference
cycles can retain their allocations, while weak links do not keep their targets alive.
[Rust Reference Cycles](https://doc.rust-lang.org/book/ch15-06-reference-cycles.html#preventing-reference-cycles-using-weakt).

As a design consequence, an address in one independently constructed graph is not a
reliable identity for the corresponding code object in another graph. Stable semantic
NodeIds still serve correspondence even if a particular implementation uses pointers
for some internal navigation. A frontend transfer likewise needs a serializable
link representation rather than a Rust process reference.

The chosen interface stores IDs in relationships and returns ordinary borrowed
references from node(id). Callers can use &CodeNode without the graph itself storing
long-lived self-references.

## Recommended semantic vocabulary

| Semantic | Direction |
| --- | --- |
| code.calls | Invocation owner → callable target |
| code.member_of | Member → semantic owner |
| code.applies_to | Implementation/extension declaration → target |
| code.inherits | Derived type → base type |
| code.implements | Implementer → contract |
| code.type_use | Referencing declaration → type |
| code.imports | Importing node → imported symbol |
| code.reads / code.writes | Accessing node → data target |

A semantic is a documented relation meaning, not an arbitrary display label. Language
implementations may add namespaced meanings as needed. No independent AccessorOf
relation is required: inline accessors can be property children; reused functions can
be linked through a suitable reference.

Declaration placement and semantic ownership remain distinct. A Rust implementation
node may contain a method while referencing the implemented type; the method can
reference its owning type through code.member_of.
[Rust Implementations](https://doc.rust-lang.org/reference/items/implementations.html).

## Comparison

The implementation derives containment facts from child-ID membership and resolves
nodes through its private index. It compares each node's own declaration, including
its enum category, separately from its children and outgoing references. A retained
child ID changing category changes that child's own declaration, not the containment
edge; child links do not carry redundant category data.

A node with only changed child membership or references can have structural/reference
differences while retaining an unchanged own declaration. Consumers decide whether to
draw an affected ancestor; the interface does not manufacture such a status.

References are identified by source node, semantic, and target meaning. Repeated
identical references are a single high-level fact. A changed target or semantic is
removal/addition. The first version does not expose occurrence counts or source
evidence coordinates.

Both graphs use the same analyzer and conventions. A failed or unsupported extraction
must not silently become an empty reference set for comparison. Explicitly unresolved
targets remain unresolved; changed resolution knowledge is not proof of a changed
runtime invocation. This graph records the facts provided by the chosen analysis.

The old graph-level relation/symbol tables, source-evidence lists, and per-field
version envelopes are no longer part of this proposal.

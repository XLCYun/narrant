---
status: accepted
---

# Encapsulate code structure analysis while preserving native declarations

Use one Review code-structure analysis module that owns language extraction,
declaration matching, relationship resolution, and analysis coverage behind a
provider-independent interface. Reuse an existing multi-language parser and structure
extractor internally, supplementing its actual gaps through language adapters; graph
and peek callers consume Review Snapshot-bound results rather than parser nodes or
language-service handles. This follows the capability modules and typed application
adapters established in ADR-0019 and ADR-0014.

The result preserves native declaration kinds, implementation groups, and all relevant
source locations. A Rust type with implementations in several files must not be forced
into a single source range or flattened into a generic class. Confirmed call targets,
candidates, incomplete analysis, unavailable content, and confirmed absence remain
distinct facts. Before and after analysis each use their own Snapshot-pinned source and
project context.

The first release uses built-in analysis and does not integrate, install, configure,
or manage language servers. Tree-sitter-based extraction does not require a language
server; any later semantic provider belongs behind the same module boundary.

Passing an extractor's current result directly to the UI would reduce initial mapping
work, but would make provider changes and language gaps spread into grouping, change
classification, navigation, and peek. The module accepts the cost of an owned
result contract and language-specific conformance examples to keep those decisions
local. It does not promise complete language semantics or select a particular dependency;
rename, modification, and presentation rules are governed by the product specification.

Related specification: [Code Structure View](../specs/0018-code-structure-changes.md).

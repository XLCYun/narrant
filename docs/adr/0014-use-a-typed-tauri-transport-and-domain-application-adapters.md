---
status: accepted
---

# Use a typed Tauri transport and domain application adapters

Frontend code accesses Tauri commands through one typed transport contract. The
contract binds every command name to its serialized arguments and result and owns
cooperative Core Git cancellation. Raw string-based `invoke` calls stay inside the
transport implementation.

Application behavior does not collect into one all-purpose Tauri adapter. Each domain
module defines the small interface its callers need, and a Tauri adapter implements
that interface using the shared transport.

Adapters remain stateless I/O implementations. Stateful ordering, cancellation, trust
gates, and atomic presentation belong to lifecycle modules above those adapters. The
Repository Entry context module owns the current Repository Entry context, Trust
transitions, Review summary synchronization, and cancellable Branch Ref loading. Its
backend Repository Entry application owns the Repository Catalog projection, availability
gate, Workspace Trust gate, complete Review projection, and canonical path lookup. React
composition code consumes the context module's interface rather than reproducing those
transitions with independent state variables.

The Review application adapter was the first migrated vertical slice. The Repository
Entry application slice now also owns its Catalog and Trust projection. The remaining
planned slices are:

- GitHub OAuth application adapter;
- Pull Request discovery application adapter.

Until those slices are extracted, their existing composition-root behavior may call
the typed transport directly. This is transitional: React modules must eventually
depend on the domain application interfaces rather than on Tauri command contracts.

The TypeScript command contract protects frontend call sites but does not prove that
Rust serialization remains synchronized. Generated Rust-to-TypeScript bindings may
replace the handwritten contract once the complete Tauri command surface has settled.

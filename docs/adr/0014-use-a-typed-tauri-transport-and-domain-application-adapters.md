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

The Review application adapter is the first migrated vertical slice. The remaining
planned slices are:

- Repository catalog and Workspace Trust application adapter;
- GitHub OAuth application adapter;
- Pull Request discovery application adapter.

Until those slices are extracted, their existing composition-root behavior may call
the typed transport directly. This is transitional: React modules must eventually
depend on the domain application interfaces rather than on Tauri command contracts.

The TypeScript command contract protects frontend call sites but does not prove that
Rust serialization remains synchronized. Generated Rust-to-TypeScript bindings may
replace the handwritten contract once the complete Tauri command surface has settled.

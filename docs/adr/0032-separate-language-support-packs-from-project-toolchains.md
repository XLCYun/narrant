# Separate Language Support Packs from project toolchains

Status: accepted

Narrant provides reusable Language Support Packs whose first consumer is Structure
Changes and whose capabilities may later be used by other language-aware features.
Rust has its own pack; TypeScript and JavaScript share a pack containing Node and
their language-service dependencies. The Rust pack supplies rust-analyzer and uses
the reviewer's existing project-compatible Rust toolchain and required components,
rather than installing or replacing Cargo, rustc or rust-src. Missing project tools
are reported explicitly. This keeps a language-capability download from silently
changing a project's toolchain selection or the reviewer's development environment.

Packs have independent versions, while each Narrant version declares the exact
pack versions it has validated. An installed matching version can be reused across
Reviews and compatible application versions. Packs do not independently track the
latest upstream tools: Narrant's language-service adapters and embedded bridges
depend on tested semantics and APIs, so a tool update requires application-level
compatibility validation.

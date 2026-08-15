---
status: accepted
---

# Name the application persistence boundary explicitly

Narrant names its application-wide SQLite boundary `persistence/`. The boundary exposes a
`ReviewStore` that persists Repositories, Workspace Trust, Reviews, Review Selections, Review
Snapshots, and File Changes. `Catalog` and `Repository Entry` are no longer current domain or
architecture terms: the durable local Git entity is a **Repository**.

Persistence owns SQLite schema migration, relational constraints, atomic writes, immutable
Snapshot records, and current-Snapshot compare-and-swap transitions. It does not own Repository
filesystem or Core Git verification, runtime availability, Workspace Trust policy, or Review
Workspace layout and cleanup. Those decisions remain in Repository Application and Review
Lifecycle.

Review Selection identity is represented explicitly. Branch Selection columns store canonical
base and head refs; Pull Request Selection columns store the GitHub repository identity and Pull
Request number. Neither representation reuses columns named for the other.

## Consequences

- `catalog/`, `RepositoryCatalog`, and `RepositoryCatalogError` become `persistence/`,
  `ReviewStore`, and `PersistenceError`.
- Public identities use `RepositoryId`, `ReviewId`, and `ReviewSnapshotId` rather than mixing
  formatted strings with SQLite row identifiers.
- Persistence results distinguish a persisted Review, its current Review Snapshot, and an
  unactivated Review Refresh Candidate.
- Repository availability and verified Git locations are application observations rather than
  persisted Repository fields.
- Rust, Tauri IPC, TypeScript, tests, and current specifications use the same terminology without
  compatibility aliases.
- A new schema migration updates current table and column names while historical migrations retain
  the schema vocabulary they originally created.

This decision supersedes the naming portions of ADR 0011 and ADR 0019. Their decisions to use
SQLite for structured Review history and to keep one application-wide persistence boundary remain
in force.

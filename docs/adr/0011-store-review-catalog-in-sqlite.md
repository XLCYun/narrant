---
status: accepted
---

# Store the Review catalog in SQLite

Narrant stores Repository Entries, Reviews, Review Selections, Review Snapshots, and
other structured review history in one SQLite database under the operating system's
application-data directory. SQLite provides atomic updates, relational integrity,
schema migration, and efficient repository and recent-review queries; secrets remain
in the operating-system credential store, while Git checkouts and future large
artifacts remain ordinary files referenced by database metadata.

## Consequences

- The application must own explicit schema migrations and transactional persistence.
- GitHub credentials are never stored in SQLite.
- Review Workspaces live under `reviews/<review-id>/repo` beside, not inside, the
  database.
- Future large reports, indexes, attachments, and exports may live in per-Review
  directories while SQLite stores their identity and location.

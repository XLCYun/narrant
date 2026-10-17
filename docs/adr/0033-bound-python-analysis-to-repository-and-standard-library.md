---
status: accepted
date: 2026-09-29
---

# Bound Python analysis to repository sources and a fixed standard library

The initial Python Structure Changes adapter uses the selected Snapshot's repository
sources and fixed standard-library declarations supplied by its Language Support Pack;
it does not discover the reviewer's Python interpreter, virtual environment or installed
third-party dependencies. Review checkouts do not reproduce those environments, and
the user chose reproducible before/after analysis over broader environment-dependent
resolution. Missing third-party context retains the existing Unresolved semantics;
this decision neither permits failed analysis to become a successful empty graph nor
changes the shared workspace and in-memory configuration rules of
[ADR-0031](0031-reuse-review-head-for-structure-analysis.md).

Implementation update (2026-10-11): [ADR-0035](0035-migrate-existing-analyzers-to-native-apis.md)
keeps this source boundary with a native Ruff/ty helper and an immutable in-memory System;
fixed typeshed is embedded in that helper. The previous ty server route is retired.
This change does not claim final Windows/CI acceptance or expand the input scope.

See the confirmed scope in the [Python specification](../specs/0023-python-language-analyzer.md).

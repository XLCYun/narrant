---
status: superseded by ADR-0006
---

# Embed local Git repository access

The desktop application must resolve refs, find merge bases, and compare committed revisions without invoking a system Git CLI. The first release uses `git2-rs` with a bundled libgit2 because its mature APIs directly cover revision resolution, merge-base calculation, tree diffing, rename detection, and blob access; this makes local review work on machines without Git and avoids behavior varying with an external executable, at the cost of native-library packaging, additional application size, and responsibility for Git compatibility testing.

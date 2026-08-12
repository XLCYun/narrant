---
status: accepted
---

# Organize the Rust backend by capability

Narrant organizes `src-tauri/src` around business capabilities and stable seams rather than global technical layers. A deep module's parent `mod.rs` is its only supported entry point; implementation files remain private, and crate visibility is not widened for test convenience. The reorganization preserves existing behavior interfaces while allowing Rust module paths to migrate without compatibility aliases.

`review/` owns Review Lifecycle, Review Selection, and Change Content while preserving their existing seams. `repository/` owns the Repository Entry Application. The application-wide SQLite persistence module remains a top-level `catalog/`, and source-neutral application operations, Core Git, and Tauri IPC/composition remain the top-level `operation/`, `core_git/`, and `tauri_app/` seams.

Large files are split by cognitive responsibility within their existing deep module, not by an arbitrary line limit. Five hundred lines triggers a review rather than mandatory splitting; workflows and transactions stay local when separating them would obscure their invariants. Tests live beside their owning module in separate test files, while only true crate-level black-box tests remain under `src-tauri/tests/`.

Migration proceeds by moving modules first, narrowing visibility and relocating tests second, extracting Tauri composition third, and only then splitting private implementations. Behavior changes and interface redesign are excluded from this reorganization and require separate decisions.

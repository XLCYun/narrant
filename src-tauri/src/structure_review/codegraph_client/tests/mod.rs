#[path = "../../../../tests/support/codegraph.rs"]
pub(crate) mod candidate;
mod python_graph;
mod python_native;
mod rust_graph;
#[cfg(unix)]
mod rust_lifecycle;
mod rust_production;
mod rust_syntax;
pub(super) mod transport;
mod typescript_graph;

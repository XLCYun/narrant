---
status: accepted
date: 2026-10-11
---

# Migrate existing analyzers to native APIs behind the common helper interface

Rust, TypeScript/JavaScript, and Python analysis will move to the common Native Helper
interface while retaining their existing capability baseline. Each helper will directly
use its analysis libraries for syntax and semantics, own the language-specific Code Graph
projection, and return normalized construction data to the shared Host. This removes the
existing LSP bridge and independent parser/server coordination instead of relocating them
inside another process; the trade-off is a larger initial migration and maintenance of
version-pinned analysis APIs.

This is the follow-up migration decision deferred by
[ADR-0034](0034-use-native-analyzer-helpers-for-new-languages.md); it replaces that ADR's
scope exclusion for existing analyzers without replacing its common process boundary.
Rust uses rust-analyzer libraries, TS/JS uses TypeScript APIs in a Node helper, and Python
uses Ruff/ty libraries in a Rust helper. An in-process TypeScript Language Service API is
compatible with this decision; starting a language-server process or retaining LSP as the
final analysis route is not.

The Host continues to own communication, process lifecycle, provenance and graph
validation, and final Code Graph construction. Direct APIs have been identified in the
upstream sources, but their existence does not establish behavioral equivalence: project
loading, target environments, source projections, cancellation, and supported platforms
still require migration-specific evidence. Rust-analyzer and ty library versions must be
kept internally consistent and pinned; no stable API guarantee is assumed.

[Specification 0025](../specs/0025-migrate-existing-analyzers-to-native-helpers.md) records
the accepted compatibility baseline: existing configuration behavior stays, equivalent
normalized printing is allowed while graph and difference semantics stay compatible, and
languages migrate and retire their old paths one at a time after acceptance. This
architecture decision does not mark implementation or acceptance complete.

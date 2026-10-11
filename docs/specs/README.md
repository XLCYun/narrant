# Specifications

This directory is the versioned source of truth for product requirements and buildable
specifications. GitHub Issues track individual implementation tickets; they do not carry
the full specification.

The TypeScript / JavaScript Code Graph is specified in
[0021 — TypeScript / JavaScript Code Graph](0021-typescript-language-analyzer.md).

On-demand Language Support Packs are being designed in
[0022 — Language Support Packs](0022-language-support-packs.md) (accepted).

Python Structure Changes support is specified in
[0023 — Python Code Graph](0023-python-language-analyzer.md) (accepted; Python 3.7–3.14,
with dedicated compatibility acceptance for 3.7–3.9).

Native helpers for Java, Go, C#, and C/C++ are specified in
[0024 — Common helper protocol](0024-native-analyzer-helper-protocol.md) and
[0024a — Language profiles and acceptance](0024a-native-language-analysis-profiles.md)
(engineering drafts; common architecture and union semantics accepted, implementation
and compatibility validation pending).

Migration of the existing Rust, TypeScript/JavaScript, and Python analyzers is specified
in [0025 — Migrate existing analyzers to Native Helpers](0025-migrate-existing-analyzers-to-native-helpers.md)
(accepted; local macOS ARM64 implementation, optimized packages, installation and Review acceptance complete;
final combined macOS/Windows acceptance and publication pending). Existing capabilities and configuration behavior remain;
helpers use direct native APIs without LSP and migrate one language implementation at a time.

## Conventions

- Name files `NNNN-short-feature-name.md`, using a zero-padded sequence number.
- Put the product problem, scope, non-goals, decisions, and cross-ticket acceptance
  criteria in the specification.
- Link every implementation issue to the governing file and, where useful, to its
  relevant heading.
- When changing requirements, update the specification first, then reconcile every open
  linked issue.
- Record durable, hard-to-reverse architecture decisions separately in `docs/adr/` and
  link them from the specification.

## Issue template

```md
## Specification

- [`docs/specs/0001-example-feature.md`](../../docs/specs/0001-example-feature.md#relevant-section)
```

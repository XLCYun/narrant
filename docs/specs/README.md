# Specifications

This directory is the versioned source of truth for product requirements and buildable
specifications. GitHub Issues track individual implementation tickets; they do not carry
the full specification.

The TypeScript / JavaScript Code Graph is specified in
[0021 — TypeScript / JavaScript Code Graph](0021-typescript-language-analyzer.md).

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

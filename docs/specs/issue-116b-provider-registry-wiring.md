# Provider registry production wiring

## Problem Statement

Desktop bootstrap and Tauri commands still instantiate Codex directly and separately prepare runtime before calling the application, preventing atomic selected-provider snapshots.

## Solution

Build both production capability bundles at startup without probing them, inject Application Settings into Babysit, and let each command delegate catalog, executable validation, and analysis selection to the registry.

## User Stories

1. As a reviewer, I want application startup to launch no coding-agent CLI, so that unused providers have no startup cost.
2. As a settings user, I want model catalog and executable validation routed to the provider I am editing, so that provider configuration remains isolated.
3. As an application owner, I want exit to release all provider resources, so that no child process survives Narrant.

## Implementation Decisions

- Bootstrap constructs the registry and settings store only; provider readiness stays lazy.
- Explain commands no longer call a separate prepare step; the application owns atomic selection and preparation.
- Catalog revisions include provider identity and are invalidated with completed explanation caches after preference changes.
- Provider selection changes do not cancel cancellable operations already running.

## Testing Decisions

- Reuse Tauri command tests for settings-triggered invalidation.
- Run focused Rust tests through the repository Cargo lock, followed by one full Rust test run before delivery.

## Out of Scope

- Any automatic provider fallback.
- Provider migration or persisted dynamic catalogs.

## Further Notes

This slice composes the registry contract into production for issue #116.

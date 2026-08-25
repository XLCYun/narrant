# Provider capability registry contract

## Problem Statement

Babysit application orchestration owns one fixed provider and contains Codex-specific runtime branches, so Application Settings cannot select a provider without leaking transport details or preparing an unselected CLI.

## Solution

Introduce a provider capability registry keyed by the stable `codex` and `claude-code` settings identities. A new analysis resolves the selected bundle once and freezes its provider, model, effort, and runtime ownership for that run.

## User Stories

1. As a reviewer, I want each new analysis to use my selected provider, so that settings control Babysit behavior.
2. As a reviewer, I want an unselected provider to remain untouched, so that a missing or signed-out CLI does not block another provider.
3. As a reviewer, I want a provider change to invalidate completed results without cancelling active work, so that old and new configurations never share output.
4. As a maintainer, I want readiness, analysis, model catalog, executable validation, and shutdown behind one provider-neutral bundle, so that orchestration contains no Codex branches.

## Implementation Decisions

- Registry keys are the stable Application Settings provider enum values, serialized as `codex` and `claude-code`.
- Construction performs no provider I/O.
- Each analysis loads settings exactly once before selecting and preparing its capability bundle.
- The prepared analysis keeps provider/runtime ownership alive until the run completes.
- Shutdown visits every registered bundle exactly once.

## Testing Decisions

- Test through the public Babysit application seam with deterministic fake bundles.
- Cover selected-provider routing, lazy readiness, provider switching, frozen in-flight analysis, cache isolation, unknown/missing registration, and shutdown.
- Do not assert private map layout or helper calls unrelated to observable behavior.

## Out of Scope

- Claude alias catalog contents and Claude module file-layout refactoring.
- Frontend settings controls.

## Further Notes

This slice implements issue #116 and ADR-0025.

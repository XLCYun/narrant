# Issue 102 Backend Slice — Babysit Model Provider

## Goal

Extend the installed Codex App Server provider so every new Line Explanation analysis
resolves and validates the current **Babysit Model Preference** against the complete,
paginated `model/list` catalog, and isolates cached explanations by the resolved model
and reasoning effort.

## Required behavior

- Parse every `model/list` page, including model id/name, `isDefault`,
  `supportedReasoningEfforts`, `defaultReasoningEffort`, hidden visibility, and cursor.
- Automatic preference resolves to the visible server default model and that model's
  default reasoning effort for each new analysis.
- Explicit preference validates the saved model and either its model-default effort or
  its explicit supported effort before analysis. Missing/unsupported selections fail
  with a stable actionable Babysit error and never fall back or rewrite settings.
- Expose a narrow typed Tauri command for the UI to fetch the dynamic visible catalog.
- The provider/model/resolved-effort/analysis-contract tuple participates in cache
  identity. Changing preference affects only later requests and does not cancel an
  already-running request.
- Do not persist the catalog and do not hard-code currently installed model ids.

## Test-first seams

- App Server provider contract fixtures cover pagination, defaults, hidden models,
  malformed/no-default catalogs, and explicit validation failures.
- Application Settings contract supplies automatic and explicit preferences without
  testing private JSON helpers.
- Cache tests prove model/effort separation and unchanged in-flight behavior.
- Rust tests live under the module's `tests/` tree; add readable Chinese comments to
  new methods, structs, and classes.

## Verification

- Run focused Rust checks/tests through `scripts/cargo-agent.sh` during development.
- Run relevant TypeScript adapter/transport tests if the command contract changes.
- Do not run the final full suite; the drone does that after integration.

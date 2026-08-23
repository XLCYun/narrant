# Issue 102 Frontend Slice — Babysit Model Settings

## Goal

Complete the Babysit section of **Application Settings** with lazy dynamic model
editing while preserving the saved **Babysit Model Preference** until an explicit,
validated save succeeds.

## Required behavior

- Normal settings display performs no App Server/model-catalog request and shows the
  saved automatic or explicit selection.
- Clicking Modify requests the current complete model catalog and presents visible
  models and only their supported reasoning efforts.
- Automatic is the default. For an explicit model, effort can be “model default” or a
  concrete supported effort; changing models resets effort to “model default”.
- Save validates against that fetched catalog and persists through the Application
  Settings adapter. Cancel, loading failure, empty catalog, or validation failure keeps
  the old preference unchanged. Errors offer retry; there is no free-text fallback.
- The dynamic catalog is never written to settings. Editing does not cancel any active
  analysis and only future analysis observes a newly saved preference.
- Use existing shadcn components where suitable and keep the settings secondary-nav
  layout responsive.

## Test-first seams

- Application shell component tests cover idle/no-fetch, lazy load, loading, retry,
  automatic/explicit edit, model-default and concrete effort, reset on model change,
  save, cancel, and persistence failure.
- Typed frontend contracts/adapters preserve the server catalog and structured errors.
- Add representative preview-harness scenarios for wide/narrow and loading/empty/error
  states without mutating business data.
- Add readable Chinese comments to new methods, structs, and classes.

## Verification

- Run focused Vitest tests, typecheck, and lint for changed frontend code.
- Do not perform the final browser pass; the drone does that after integration.


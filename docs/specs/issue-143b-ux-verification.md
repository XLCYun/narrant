# Issue 143B: Custom UX automation and visual verification

## Objective

Complete non-live frontend verification and reviewer documentation for the Custom
provider without changing backend runtime files owned by Issue 143A.

## Scope

- Cover selecting Custom, complete connection editing, password reveal/hide, local
  validation, saving, save failure with retained values, and unconfigured recovery.
- Extend the reusable Preview Harness when needed for empty, configured, invalid,
  saving, failed, password-hidden, password-revealed, and unconfigured recovery states.
- Use `pnpm dev:preview` and the in-app browser to inspect wide and narrow viewports.
- Verify screenshots, accessible DOM, password input type and visibility action, retained
  errors/values, relevant element dimensions, and absence of horizontal overflow.
- Restore the default viewport when browser inspection finishes.
- Add an opt-in smoke-test document for reviewer-supplied OpenAI-compatible and
  Anthropic-compatible endpoints. It must be explicitly excluded from CI and must never
  print or record a key, full endpoint URL, prompt, source, response, explanation, or
  repository tool result.
- Remove obsolete frontend slice seams and placeholders encountered within this scope.

## Constraints

- Prefer existing shadcn components; do not create a new primitive when one exists.
- Add Chinese comments for methods, structs, and classes.
- Do not edit `src-tauri/src/babysit/custom/` or backend integration tests.
- Do not make any live model request and do not manually trigger quality-gate.

## Verification

- Run affected frontend tests, full frontend tests, lint, typecheck, and production build
  as appropriate for this slice.
- Record browser viewport sizes, scenarios, DOM assertions, dimensions, overflow result,
  and screenshot paths.
- Report exact commands, counts, residual risks, commit, and changed files.

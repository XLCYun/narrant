# Task 4 Fix Round 2 Report

## Result

The remaining preference-concurrency finding is fixed. Rust snapshot sequence is again the sole
authority for accepted update state, while the current preference intent exists only as a reversible
display overlay.

Implementation commit: `aaa679b54eeda04d9001d840f3feccd406fb6528`
(`fix: preserve authoritative update preference ordering`).

## Finding mapping

- `src/application-update/use-application-update.ts`
  - Separates `authoritativeSnapshotRef` from `displayedSnapshotRef` and the current optimistic
    `preferenceIntentRef`.
  - Every preference command resolve now enters the same unconditional monotonic `accept` path. A
    frontend request token can no longer discard a higher Rust sequence returned by an older request.
  - Accepted authority retains the complete snapshot, including `preferenceWarning`; an in-flight latest
    intent overlays only `automaticDownload` for display.
  - The request token controls only the current optimistic rollback, pending state, and command error.
    A current reject publishes the latest authoritative value, while an older reject cannot alter newer
    state.
  - A current reject reports “not saved and restored” only when authority has not advanced since that
    request began. A later authoritative event or successful command clears the obsolete command error.
- `tests/application-update-controller.test.tsx`
  - Covers A(false) + B(true) both rejecting from authoritative `true`, finishing at sequence 1 / `true`
    rather than A's optimistic `false`.
  - Covers B returning sequence 4 before old-token A returns sequence 5; sequence 5 is accepted and wins.
  - Repeats both reject and sequence-4/sequence-5 orderings with an intervening authoritative event and
    durability/state-unknown warnings, asserting displayed value, sequence, pending, warning, and error.
  - Proves a later warning event and a later successful old request each clear an already visible
    preference command error and preserve their authoritative warning.

## Verification

- Focused Task 4 tests:
  - `pnpm exec vitest run tests/application-update-controller.test.tsx tests/application-update-components.test.tsx tests/tauri-application-update-adapter.test.ts tests/tauri-transport.test.ts`
  - Result: 4 files, 45 tests passed.
- Affected Task 4 tests:
  - `pnpm exec vitest run tests/application-update-controller.test.tsx tests/application-update-components.test.tsx tests/tauri-application-update-adapter.test.ts tests/tauri-transport.test.ts tests/settings-shell.test.tsx tests/home-page.test.tsx tests/App.test.tsx tests/application-language-ui.test.tsx`
  - Result: 8 files, 118 tests passed.
- `pnpm exec tsc --noEmit`: passed.
- `pnpm lint`: passed with zero lint warnings/errors.
- `pnpm build`: passed; 2032 modules transformed.
- `git diff --check`: passed.
- Environment note: pnpm reported local Node `v26.5.0` is above the repository's declared `<26` range;
  the requested commands still passed.
- `quality-gate` was not run.

## Residual risk

No known Task 4 preference-concurrency gap remains. The normal Settings Switch still prevents ordinary
repeat clicks while pending, while the public hook is now correct even when callers issue concurrent
preference intents directly.

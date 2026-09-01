# Task 4 Report: Frontend Application Update experience

## Result

Task 4 is complete. The frontend observes the Rust-owned Application Update capability through five
narrow typed commands and the single `application-update:snapshot` event. No frontend API accepts an
endpoint, URL, package path, signature, target, install kind, or release channel.

Implementation commit: `2703213` (`feat: add application update experience`).

## Files and requirement mapping

- `src/application-update/application-update.ts`
  - Defines the provider-neutral snapshot, state, candidate, progress, failure and warning contract.
  - Defines the narrow snapshot/manual-check/preference/retry/restart port.
- `src/application-update/use-application-update.ts`
  - Subscribes before reading the initial snapshot to close the startup race.
  - Accepts only strictly increasing sequences and discards stale/duplicate events and command results.
  - Implements optimistic automatic-download preference updates, conditional rollback on IPC failure,
    and authoritative Rust warning/snapshot convergence.
- `src/infrastructure/tauri-application-update-adapter.ts`
  - Maps the narrow port to the five Rust typed commands and one snapshot event.
- `src/infrastructure/tauri/command-contract.ts`
  - Adds all five update commands to the typed contract and inventory synchronized with Rust bootstrap.
- `src/components/settings-page/application-update-settings.tsx`
  - Shows current/candidate versions, all seven states, automatic-download Switch, manual check,
    retryable failure, recovery/preference warnings, retry and detail actions.
- `src/components/application-update/application-update-attention.tsx`
  - Shows the 32 px circular accessible attention icon only for available/downloading/failed/staged.
  - Click, Enter and Space only open the in-app surface.
- `src/components/application-update/application-update-surface.tsx`
  - Shows candidate version, safe Markdown/GFM notes, determinate/indeterminate progress, Retry, Later,
    and staged-only Restart and Update.
  - Uses `skipHtml`, blocks remote image loading, filters non-HTTP(S) links, constrains long notes,
    fixes initial focus on Later, closes on Escape, and restores focus to the opener.
- `src/components/ui/switch.tsx`, `src/components/ui/progress.tsx`
  - Uses the repository's configured shadcn/Base UI primitives.
- `src/components/home-page.tsx`, `src/App.tsx`
  - Composes the icon immediately after GitHub Identity and before Settings; owns one global surface.
- `src/components/settings-page/settings-page.tsx`, `src/settings/settings-section.ts`
  - Adds the Application Update settings destination.
- `src/lib/i18n.ts`
  - Adds natural zh-CN and en-US update states, actions, warnings, errors and accessibility labels.
- `src/dev/preview.tsx`
  - Adds reusable settings, wide/short, narrow/long, known/unknown progress, failed and staged fixtures.
- `tests/application-update-controller.test.tsx`
  - Covers subscribe-before-read, stale/duplicate rejection, optimistic rollback and durability warning.
- `tests/application-update-components.test.tsx`
  - Covers every visible state, icon visibility, keyboard opening, known/unknown progress, safe GFM,
    retry/restart intent separation and English copy.
- `tests/tauri-application-update-adapter.test.ts`, `tests/tauri-transport.test.ts`
  - Cover event/command narrowing and frontend/Rust command inventory synchronization.

## Automated verification

- Focused and affected Vitest:
  - `pnpm exec vitest run tests/application-update-controller.test.tsx tests/application-update-components.test.tsx tests/tauri-application-update-adapter.test.ts tests/tauri-transport.test.ts tests/settings-shell.test.tsx tests/home-page.test.tsx tests/App.test.tsx tests/application-language-ui.test.tsx`
  - Result: 8 files, 103 tests passed.
- `pnpm exec tsc --noEmit`: passed.
- `pnpm lint`: passed with zero warnings/errors.
- `pnpm build`: passed (2032 modules transformed).
- `git diff --check`: passed.
- Environment note: pnpm reported that local Node `v26.5.0` is above the repository's declared
  `<26` range; all requested checks still passed.
- `quality-gate` was not triggered.

## In-app browser acceptance evidence

Preview command: `pnpm dev:preview`; target: the reusable `preview.html` harness on
`http://127.0.0.1:1421`. The in-app browser viewport override was reset after acceptance.

### Wide and sidebar composition

- Viewport: 1440 x 900.
- Dialog bounds: x 376.5, y 272, 672 x 356.
- Attention icon bounds: x 187, y 845, 32 x 32.
- GitHub Identity bounds: x 12, y 834, 167 x 54; identity-to-icon gap: 8 px.
- Sidebar: 279 px wide. Body `scrollWidth === clientWidth`; no horizontal overflow.
- Accessibility DOM exposed dialog name “应用更新”, version, “版本说明”, Later and Close.
- Screenshot:
  `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/wide-available.png`

### Narrow dialog and long content

- Viewport: 390 x 844.
- Dialog bounds: x 16, y 114.6, 343 x 614.8.
- Release Notes viewport: 380 px client height / 906 px scroll height; vertical scrolling is contained.
- Body has no horizontal overflow. Dangerous link count is zero; raw HTML did not execute; remote
  images are not created.
- Initial focus is Later and notes start at `scrollTop=0`, so long linked notes do not jump downward.
- Screenshot:
  `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/narrow-long-notes.png`

### Narrow desktop sidebar and long identity

- Viewport: 800 x 844.
- Sidebar: 279 px; footer: 255 x 54.
- Long identity text: 99 px client width / 394 px scroll width, visibly truncated without overlap.
- Attention icon remains 32 x 32 and 8 px immediately after Identity; body has no horizontal overflow.
- Screenshot:
  `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/narrow-800-sidebar-long-identity.png`

### Progress, failure, staging and settings

- Determinate, viewport 1000 x 800: `aria-valuenow=7340032`, `aria-valuemax=12582912`, accessible
  label “更新已下载 58%”; no body overflow.
  - `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/downloading-determinate.png`
- Indeterminate, viewport 1000 x 800: no fake progressbar; status label says total size is unknown and
  reports 3.0 MB downloaded.
  - `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/downloading-indeterminate.png`
- Failed: Retry is present, Restart is absent, stable localized error is exposed, and Retry transitions
  the fixture into checking without closing the surface.
  - `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/failed-retry.png`
- Staged: Later and Restart and Update are present; Retry is absent; dialog is 672 x 356 with no overflow.
  - `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/staged-restart.png`
- Settings: Application Update region, current version, up-to-date status, named Switch and manual check
  are present. Switch changed true to false; manual check exposed checking status and disabled action.
  - `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/settings-up-to-date.png`

### Keyboard and focus

- Enter on the focused attention icon opens the dialog and moves focus to Later.
- Escape closes after the dialog transition and restores focus to the attention icon.
- Later closes the surface without sending restart; the close icon has accessible name “关闭”.

## Residual risks

- No Task 4 functional gaps remain.
- Native updater installation and real restart behavior are Rust/CI responsibilities outside this
  frontend task; this UI deliberately sends only the narrow restart intent.
- Browser screenshots are local ignored evidence files at the absolute paths above; this report is
  committed, while binary screenshots remain out of source control.

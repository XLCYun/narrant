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
  label “更新已下载 58%”; the actual dialog is visible at 672 x 416, initial focus is Later, and
  body `scrollWidth === clientWidth` (985 px).
  - `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/downloading-determinate-real.png`
- Indeterminate, viewport 1000 x 800: no fake progressbar; status label says total size is unknown and
  reports 3.0 MB downloaded.
  - `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/downloading-indeterminate.png`
- Failed: Retry is present, Restart is absent, stable localized error is exposed, and Retry transitions
  the fixture into checking without closing the surface.
  - `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/failed-retry.png`
- Staged: Later and Restart and Update are present; Retry is absent; dialog is 672 x 356 with no overflow.
  - `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/staged-restart.png`
- Settings uses the real `ApplicationSettingsDestination` with all four tabs and the responsive settings
  shell. The Application Update tab can be reached from Appearance and exposes current version,
  up-to-date status, the named Switch, and manual check.
  - Wide 1440 x 900: settings shell 1361 x 812, update region 1007 x 302, tablist 240 x 640,
    body `scrollWidth === clientWidth` (1425 px).
    `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/settings-real-wide.png`
  - Narrow 800 x 844: update region 414 x 322, body 800 / 800 with no horizontal overflow.
    `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/settings-real-narrow.png`
  - Phone 390 x 844: settings shell 335 x 624, update region 293 x 386, horizontal tablist
    335 x 48 (`scrollWidth=339`), body 375 / 375 with no horizontal overflow.
    `/Users/lizhanfa/projects/narrant/.worktrees/application-updates/.superpowers/sdd/2026-09-01-application-updates/task-4-browser/settings-real-phone.png`

### Keyboard and focus

- Enter on the focused attention icon opens the dialog and moves focus to Later.
- Escape closes after the dialog transition and restores focus to the attention icon.
- Later closes the surface without sending restart; the close icon has accessible name “关闭”.

## Fix round 1 (2026-09-02)

Implementation commit: `d33798d` (`fix: harden application update UI recovery`).

### Review finding mapping

- Preference authority and command races:
  - Preference requests now carry a monotonic frontend request token in addition to the Rust snapshot
    sequence. An older command resolve/reject cannot overwrite a later request.
  - A rejected command sets the “not saved and restored” error only when the same baseline sequence and
    optimistic value are still current and an actual rollback occurs. If a higher-sequence event has
    already been accepted, its `automaticDownload` and `preferenceWarning` remain authoritative.
  - Deferred tests cover event-first/reject, event-first/late resolve, and multiple request results that
    complete out of order.
- Retry and Restart command failures:
  - The update dialog now exposes localized retry/restart command failures with `role="alert"` and
    `aria-live="assertive"`. Starting the next action clears the old failure, while rejection re-enables
    the focused button.
  - Controller/surface integration tests cover both rejects and prove both commands still receive no
    frontend payload.
- Real Settings preview and visual evidence:
  - The reusable Application Update fixture is injected into the real `ApplicationSettingsDestination`
    with `initialSection="application-update"`; the preview no longer bypasses the real fourth tab or
    responsive settings shell.
  - The three Settings screenshots and measurements above replace the former isolated-card evidence.
    The real determinate dialog screenshot also replaces the previous image that did not show a dialog.
  - The existing 390 x 844 long-notes evidence remains valid and unchanged.
- Load recovery and lifecycle:
  - The hook exposes `reload`, which tears down an existing listener, re-subscribes, and reloads the
    snapshot. Both empty and previously loaded failure states show an executable Reconnect action.
  - Tests cover subscribe rejection then recovery, snapshot rejection then recovery, listener cleanup,
    and StrictMode duplicate mount/unmount without double listeners or stale async writes.
- Keyboard behavior:
  - Real WebView acceptance showed that the Base UI button did not reliably synthesize click from
    Enter/Space. The attention button therefore keeps a narrow explicit key handler; `preventDefault`
    prevents native double activation, and tests assert exactly one open for Enter and Space.

### Round 1 automated verification

- Final affected Vitest command (same eight files listed above): 8 files, 113 tests passed.
- Final focused keyboard/component run: 1 file, 22 tests passed.
- `pnpm exec tsc --noEmit`: passed.
- `pnpm lint`: passed with zero warnings/errors (apart from the repository Node engine notice).
- `pnpm build`: passed; 2032 modules transformed.
- `git diff --check`: passed.
- `quality-gate` was not triggered.

### Round 1 browser verification

- The worker task's in-app browser backend was unavailable, so the main task used its in-app browser
  binding against the same running `pnpm dev:preview` harness and supplied the evidence recorded here.
- Real Settings navigation and responsive layout passed at 1440 x 900, 800 x 844, and 390 x 844; the
  accessibility DOM exposed the actual four-tab tablist and the selected Application Update region.
- The determinate scenario rendered a real 672 x 416 dialog at 1000 x 800 with the correct progress DOM,
  visible 58% value, initial Later focus, and no body overflow.
- At 1440 x 900, focused attention Enter and Space each opened the dialog exactly once and moved focus to
  Later. Escape closed it and restored focus to the attention button named “有应用更新可用”.
- The viewport override was reset and the browser tab was closed after acceptance.

## Residual risks

- No Task 4 functional gaps remain.
- Native updater installation and real restart behavior are Rust/CI responsibilities outside this
  frontend task; this UI deliberately sends only the narrow restart intent.
- Browser screenshots are local ignored evidence files at the absolute paths above; this report is
  committed, while binary screenshots remain out of source control.

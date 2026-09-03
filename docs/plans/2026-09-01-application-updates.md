# Application Updates Implementation Plan

**Spec:** [`docs/specs/0017-application-updates.md`](../specs/0017-application-updates.md)

## Global Constraints

- Implement the complete staged lifecycle in the governing spec; do not substitute direct
  frontend use of the stock Tauri updater plugin or a check-only MVP.
- Stable metadata is read from
  `https://updates.cyunlab.com/narrant/channels/stable/latest.json` and supports
  exactly `darwin-aarch64` and `windows-x86_64` initially.
- Endpoint, public key, package URL, signature and install path remain Rust-owned. Frontend
  callers can only observe snapshots and send the narrow inputs named by the spec.
- Narrant owns one updater minisign trust root. The private key/password never enter source,
  frontend bundles, public Release metadata or logs.
- Ordinary exit and Update Restart share one idempotent application-stop result and one total
  fifteen-second deadline. Timeout skips installation, retains staging and permits exit.
- The first updater-capable Release is a one-time manual bootstrap; later releases require
  real previous-Stable-to-candidate smoke on both platforms.
- The public Release remains mutable by GitHub administrators because immutable releases are
  explicitly out of scope; automation itself must never alter a published Release.
- Follow the repository's Chinese-comment rule for methods, structs and classes. Rust private
  module tests live in module `tests/` directories or sibling test files, never inline.
- All Cargo commands go through `scripts/cargo-agent.sh`; do not share divergent target dirs,
  disable incremental builds, clean caches, or manually trigger `quality-gate`.
- UI work must use existing shadcn components when suitable and must be verified in the Vite
  Preview Harness with wide/narrow, long/short, progress, failed, and staged states using the
  in-app browser. Browser checks complement, not replace, tests/typecheck/lint/build.
- Preserve unrelated user work, including `docs/research/common-software-licenses.md` in the
  original checkout.

## Task 1: Build the shared application-stop boundary

### Goal

Make ordinary exit and Update Restart able to reuse one bounded, idempotent cleanup result.

### Files

- Modify `src-tauri/src/operation/cancellable.rs` and its separate tests.
- Add `src-tauri/src/tauri_app/shutdown.rs` plus tests under
  `src-tauri/src/tauri_app/tests/`.
- Modify `src-tauri/src/tauri_app/mod.rs` and `bootstrap.rs` only for composition/event wiring.
- Adjust Review/Babysit lifecycle seams only where the shared deadline requires a truthful
  completion result.

### Requirements

1. Atomically close operation admission, cancel every Running entry, and await Running-entry
   count zero using notification rather than polling. Early-cancellation records do not block drain.
2. New operations after admission closes return a stable cancelled/shutting-down error.
3. Add a managed application-stop object whose first caller runs cancel/drain, Babysit shutdown,
   Review shutdown and telemetry shutdown under one fifteen-second deadline. Concurrent or repeated
   callers await/reuse the same result.
4. Intercept `ExitRequested`/main-window close without blocking the event loop, then perform one
   internal final exit. The following `Exit` event cannot repeat cleanup.
5. Expose an Update Restart intent without importing updater internals into the stop object.
6. Unit/integration tests cover admission close, drain success, timeout, repeated events, step order,
   shared outcome and “timeout still exits but is not installable”.

### Verification

- `scripts/cargo-agent.sh test --lib operation`
- Focused Tauri application shutdown tests through `scripts/cargo-agent.sh test --lib <filter>`.

## Task 2: Implement the Application Update core and durable staging

### Goal

Create a framework-independent state machine and disk boundary for the complete policy.

### Files

- Add `src-tauri/src/application_update/mod.rs`, focused production files, and separate
  `src-tauri/src/application_update/tests/` fixtures/tests.
- Add only the persistence dependencies needed by the spec to `src-tauri/Cargo.toml`.

### Requirements

1. Model `idle/checking/up_to_date/available/downloading/staged/failed` snapshots with monotonic
   sequence and provider-neutral fields.
2. Support startup check, six-hour periodic tick, manual check, automatic-download preference,
   explicit retry and Update Restart request. Coalesce concurrent checks.
3. Enforce SemVer increase, default automatic download, persisted preference, three retryable
   attempts with 1/2/4-second backoff, and stable retryable/non-retryable failures.
4. Stream to a temporary file, fsync, verify the Tauri minisign signature, and atomically persist
   one `package.bin` plus metadata. Never load an unbounded package into memory.
5. Reverify on recovery and before install; cover interrupted writes, tampering, target/version
   mismatch, higher-candidate replacement and cleanup after the running version reaches staging.
6. Keep time, network metadata, verifier and installer behind injected ports so state-machine tests
   are deterministic and disk tests exercise real files.

### Verification

- `scripts/cargo-agent.sh test --lib application_update`
- `scripts/cargo-agent.sh check --lib`

## Task 3: Add the trusted Tauri and platform adapters

### Goal

Connect the core to Tauri updater metadata, HTTPS download, macOS/Windows installation and the
shared application-stop boundary.

### Files

- Add Application Update adapter/platform files and separate tests under
  `src-tauri/src/application_update/`.
- Add `src-tauri/src/tauri_app/commands/application_update.rs` and register typed commands/state.
- Modify `src-tauri/src/tauri_app/bootstrap.rs`, `src-tauri/src/tauri_app/commands/mod.rs`,
  `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, platform Tauri configs and capabilities.

### Requirements

1. Add compatible `tauri-plugin-updater`, rustls HTTPS, minisign, temp-file/archive dependencies;
   enable `bundle.createUpdaterArtifacts: true` and only NSIS/DMG shipping targets.
2. Parse compile-time production endpoint/public key fail closed. Configure both plugin and custom
   updater builder with the same values and a bounded manifest request.
3. Download only the update returned by the trusted builder; reject caller URLs/paths/signatures.
4. Port/adapt DSH's proven macOS rollback replacement and current-user NSIS update/relaunch
   semantics for Narrant's two targets, including installation-before-exit ordering.
5. Persist redacted updater diagnostics without package signature, credentials or user paths.
6. Publish one snapshot event and commands for snapshot/manual check/preference/retry/restart.
7. Normal exit installs only after successful shared stop; timeout skips installation. Explicit
   restart uses the same stop outcome and relaunches only after successful installation.
8. Tests cover config failure, HTTPS allowlist, real disk signature verification, target/install-kind
   validation, macOS rollback, Windows arguments, cleanup-before-install and timeout behavior.

### Verification

- `scripts/cargo-agent.sh test --lib application_update`
- `scripts/cargo-agent.sh check --lib`
- Regenerate/verify Tauri capability schemas only through the repository's normal Tauri tooling.

## Task 4: Implement the frontend Application Update experience

### Goal

Expose update settings and attention states without leaking the Rust trust boundary.

### Files

- Add a narrow frontend application-update port/state/controller and Tauri adapter.
- Extend the typed Tauri command inventory and transport sync tests.
- Add Settings section components and update surface using existing shadcn primitives.
- Modify the home sidebar identity/footer composition for the circular attention icon.
- Extend i18n resources, component tests and `src/dev/preview.tsx` representative scenarios.

### Requirements

1. Observe one monotonic snapshot source and discard stale sequence values.
2. Settings shows current version, automatic-download toggle, manual check, state and retryable error.
3. A small circular accessible icon sits immediately right of GitHub Identity only for
   available/downloading/failed/staged; clicking opens the in-app update surface and never directly
   restarts.
4. The surface renders version, Markdown/GFM Release Notes, determinate/indeterminate progress,
   retry, Later and Restart and Update. It cannot accept arbitrary trust inputs.
5. Preserve sidebar behavior at narrow widths and long identity/release text; use clear zh-CN/en-US
   strings and keyboard/focus behavior.
6. Component tests cover every state, stale events, progress, preference rollback on error,
   retry and restart intent. Preview fixtures cover wide/narrow, long/short notes, determinate/
   indeterminate download, failed and staged states.

### Verification

- Focused Vitest component/transport tests.
- `pnpm exec tsc --noEmit`
- `pnpm lint`
- `pnpm build`
- In-app browser visual acceptance with explicit wide and narrow viewports, screenshots,
  accessibility DOM and measured icon/dialog bounds; restore the default viewport afterward.

## Task 5: Produce signed cross-repository Draft updater assets

### Goal

Extend private-source release automation to create a complete, internally consistent public Draft.

### Files

- Modify `.github/workflows/release.yml`.
- Add focused release asset/manifest/reconciliation scripts under `scripts/` with tests under
  `scripts/tests/`.
- Update release-version checks only if needed to keep all declarations aligned.

### Requirements

1. Production builds require `TAURI_SIGNING_PRIVATE_KEY`, its password, endpoint and public key;
   fail before build when missing.
2. Rebuild/sign the updater archive from the final signed/notarized macOS app. Collect current-user
   Windows NSIS updater `.exe` and signature. Use the exact asset names in the spec.
3. Generate `latest.json` with exact two-target set, tag-specific public URLs and literal signature
   contents; omit Draft `pub_date`; copy public Release Notes into `notes`.
4. Generate SHA256SUMS after the full asset set is staged. Reject unexpected/missing/duplicate
   assets and invalid signatures before upload.
5. Reconcile only Draft Releases in `cyunlab/narrant`; never modify published Releases or expose
   private SHA/link in public title, notes, manifest or asset names.
6. Tests cover version/target/URL/signature/asset validation, deterministic manifest generation,
   retries against Draft state and rejection of published state.

### Verification

- `pnpm test:scripts`
- Static workflow validation plus focused script fixtures.

## Task 6: Gate Stable promotion with native upgrade evidence

### Goal

Make human approval release a candidate only after GitHub-only DSH-grade validation.

### Files

- Add `.github/workflows/update-smoke.yml` and a protected manual promotion workflow.
- Add native smoke driver, two platform adapters, TLS gate, evidence generator/verifier and tests
  under `scripts/` and `scripts/tests/`.
- Add `docs/application-update-operations.md`.

### Requirements

1. Download the Draft through the public-repository API token and byte-verify the exact manifest,
   packages, signatures and asset digests.
2. Serve only exact GitHub latest/tag asset paths from a temporary trusted HTTPS gate; install its
   ephemeral CA only inside the runner and restore host/DNS state even on failure.
3. On macOS arm64 and Windows x64, install/launch the previous updater-capable Stable, drive the real
   application through check/download/stage/normal-exit/install/relaunch and record checkpoints.
4. For the bootstrap release, replace previous-version upgrade with signed install/launch/config/
   staging-recovery checks and mark evidence `bootstrap`; later promotion rejects bootstrap evidence.
5. Evidence binds source tag/full SHA, public Release ID/tag, Draft asset IDs/digests, manifest SHA,
   previous Stable manifest SHA, OS/arch, freshness and real-native checkpoints. Attest and aggregate
   exactly two targets.
6. Promotion runs in a protected private-repository Environment after human approval, re-fetches the
   Draft and evidence, rejects stale/mismatched/incomplete input, then publishes/marks latest through
   `CYUNLAB_RELEASE_TOKEN`.
7. The runbook covers required secrets/variables, one-time bootstrap, failure recovery, private-key
   backup/loss/leak and old-key-signed bridge rotation. It explicitly says immutable releases are not
   enabled and automation never edits published assets.

### Verification

- Focused Node script tests for TLS routing, evidence schema/digests/freshness, bootstrap transition,
  exact target aggregation and promotion rejection.
- Workflow fixtures/static assertions prove permissions, environments, cross-repository identity and
  no `release.published` single-repository assumption.

## Task 7: Close cross-slice acceptance gaps

### Goal

Prove the assembled feature satisfies the governing spec and repository gates without manually
triggering `quality-gate`.

### Files

- Add or adjust only cross-slice tests/fixtures and documentation gaps revealed by verification.
- Do not redesign production modules in this task; structural defects return to their owning task.

### Requirements

1. Audit every acceptance criterion in the spec against a named test, source contract, workflow
   assertion, browser observation or runbook section.
2. Run focused frontend/Rust/script verification, then exactly once run the Issue-level full Rust
   test through `scripts/cargo-agent.sh`.
3. Run TypeScript tests, typecheck, lint, production build and `git diff --check`.
4. Run Preview Harness visual acceptance for explicit wide/narrow viewports and all required content/
   dialog/progress states; capture screenshots, accessibility DOM and actual bounds, then restore the
   default viewport.
5. Record any native smoke that can only execute on CI as unexecuted local evidence with the exact
   workflow contract that gates publication; do not claim a remote run occurred.
6. Update the spec only if implementation uncovered a genuine requirement correction; architecture
   changes require a corresponding ADR update.

### Verification

- `pnpm test`
- `pnpm test:scripts`
- `pnpm exec tsc --noEmit`
- `pnpm lint`
- `pnpm build`
- `scripts/cargo-agent.sh check --lib`
- Focused Rust tests as needed, followed by one final `scripts/cargo-agent.sh test --lib`
- `git diff --check`

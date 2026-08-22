# Babysit Mode

## Problem Statement

Reviewers can inspect exact before-and-after File Change content, but understanding an
unfamiliar codebase still requires reconstructing the purpose of individual lines and
their surrounding repository context. Babysit Mode should explain code without turning
Narrant into an automated reviewer or modifying the code being reviewed.

## Solution

Babysit Mode lets a reviewer ask an installed Codex runtime to explain the current
File Change. Codex receives the complete before-and-after text as one analysis batch,
may explore the trusted Review Workspace through a restricted read-only sandbox, and
returns a short explanation for every non-empty line. Narrant validates the complete
response, caches it only for the current process, and presents explanations above the
corresponding Monaco lines while preserving the immutable Review Snapshot.

## User Stories

1. As a reviewer unfamiliar with a codebase, I want a short explanation for each code
   line, so that I can understand the change without reconstructing every local detail.
2. As a reviewer, I want Babysit Mode to explain code rather than judge it, so that its
   output does not masquerade as review findings or approval advice.
3. As a reviewer, I want analysis pinned to the exact Review Snapshot I opened, so that
   a refresh cannot mix explanations from different revisions.
4. As a reviewer, I want one File Change analyzed as a batch, so that Codex understands
   relationships among its lines without one model request per line.
5. As a reviewer, I want both before and after content explained, so that I can
   understand removed behavior as well as newly introduced behavior.
6. As a reviewer, I want explanations for every non-empty eligible line, so that an
   unchanged line can still be understood when I request the complete view.
7. As a reviewer, I want changed-line explanations shown by default, so that Babysit
   Mode remains focused on the review rather than overwhelming the Diff.
8. As a reviewer, I want to reveal all generated explanations without another Agent
   run, so that I can inspect surrounding code without additional delay or usage.
9. As a reviewer, I want explanations displayed above their source lines, so that the
   relationship remains visible in both original and modified editors.
10. As a reviewer, I want side-by-side and inline Diff layouts to preserve code
    alignment, so that expanded explanations do not make the comparison misleading.
11. As a reviewer, I want source code to remain unchanged, so that AI output cannot
    pollute the File Change or disposable Review Workspace.
12. As a reviewer, I want the original Diff to remain readable while Codex works, so
    that optional analysis never blocks ordinary review.
13. As a reviewer, I want generation and repair progress in the Diff footer, so that I
    can understand what the current file is waiting for.
14. As a reviewer, I want to cancel a running analysis, so that switching files or
    changing my mind does not leave unnecessary work running.
15. As a reviewer, I want failed analysis to offer retry and redacted diagnostics, so
    that I can recover without exposing repository content.
16. As a reviewer, I want completed explanations reused while the Review stays open,
    so that hiding and reopening Babysit Mode does not call Codex again.
17. As a reviewer, I want stale and partial output rejected, so that late, cancelled,
    timed-out, or structurally incomplete results never appear as valid explanations.
18. As a reviewer, I want Codex to repair a malformed response automatically, so that
    transient omissions do not immediately require manual retry.
19. As a reviewer, I want unsupported files identified before analysis starts, so that
    lock files, generated content, and non-text changes do not waste Agent work.
20. As a reviewer with Codex already installed, I want Narrant to reuse my Codex
    environment and authentication, so that I do not configure another model account.
21. As a reviewer, I want Codex discovered only when I enable Babysit Mode, so that the
    feature adds no startup process or latency when unused.
22. As a reviewer whose Codex executable is outside the application PATH, I want to
    select it with a native picker, so that desktop launch environments still work.
23. As a reviewer, I want actionable recovery when Codex is absent, signed out, or
    incompatible, so that failures identify installation, login, or upgrade steps.
24. As a reviewer, I want Babysit analysis threads removed from Codex history, so that
    an internal product operation does not clutter my interactive sessions.
25. As a maintainer, I want Codex and future Claude Code providers to satisfy one
    contract, so that provider protocols cannot leak into Review Reader behavior.
26. As a reviewer, I want to regenerate completed Line Explanations for the current
    File Change, so that I can request a fresh analysis without closing Babysit Mode
    or discarding cached explanations for other files.
27. As a maintainer, I want CI to use a deterministic fake App Server, so that tests do
    not require a personal login, network access, or paid model request.

## Scope

- The Review Reader exposes an optional Babysit Mode for the currently selected text
  File Change in one exact Review Snapshot.
- Enabling the mode asks a backend Babysit Agent Provider to analyze the whole File
  Change as one batch.
- The analysis covers every non-empty line on both the before and after sides. The
  Agent may explore the trusted Review Workspace read-only to understand repository
  context.
- The Review Reader initially shows explanations only above changed non-empty lines. A
  toggle reveals explanations above every analyzed line without starting another run.
- Line Explanations help the reviewer understand code. They do not judge quality,
  report findings, recommend review decisions, or modify source text.

## Implementation Decisions

- A deep backend Babysit module owns eligibility, File Change input construction,
  provider orchestration, structured-output validation, the repair loop, cancellation,
  thread cleanup, timeouts, and process-local caching behind one file-analysis
  interface.
- The frontend Babysit module owns Review Reader session state, progress presentation,
  Monaco filtering and view zones, retry and cancellation actions, and stale-request
  rejection behind one Review Reader binding.
- File analysis carries a provider-independent `reuse_cached` or `regenerate` policy.
  Regeneration bypasses and replaces only the exact cached File Change result; Retry
  remains the recovery action for a failed analysis.
- Tauri transport remains a typed adapter. A per-request progress channel carries only
  provider-independent phases, while the command returns the final validated result.
- The Codex adapter owns executable discovery, App Server lifecycle, JSONL protocol,
  capability probing, authentication status, sandbox configuration, and thread
  deletion. Its protocol types do not escape the provider seam.
- The provider seam is real because production Codex, a deterministic test adapter, and
  future Claude Code behavior must satisfy the same externally observable contract.

### Analysis Contract

- Narrant creates one provider-independent request for the entire File Change. The
  request contains both file sides and a complete inventory of Narrant-owned line
  targets; it never invokes the provider once per line.
- A line target identifies its before-or-after side, one-based line number, and
  non-empty source text. The provider returns one short explanation for every
  target in one structured response. The response is a compact JSON object whose
  `before` and `after` members map line-number strings directly to explanations; it
  never repeats or annotates the source text.
- The backend does not classify changed lines for the provider. The Monaco DiffEditor
  owns the presented line comparison and uses its computed line changes to filter the
  complete explanation set. Switching the filter never starts another Agent run.
- Explanations are plain text, normally one or two short sentences, and no longer than
  240 Unicode characters. They preserve source identifiers and use the current UI
  locale.
- Narrant validates target identity, exact coverage, uniqueness, non-empty text, length,
  and control characters. A failed validation is returned to the same Agent thread for
  repair up to three times. After the third failed repair, the run fails and the user
  may retry manually.
- Stable user-visible run phases are `generating`, `repairing`, and `completed`.
  Failed and cancelled runs end in `failed` and `cancelled` outcomes rather than
  introducing provider-specific progress phases. Provider-specific tool, message, and
  token events do not cross the application seam.

### Provider Decisions

- Installed Coding Agent Providers and Built-in Agent Providers implement the same
  contract described above.
- The first vertical slice requires an installed Codex executable and launches a
  Narrant-owned `codex app-server` child process. It reuses the user's Codex
  authentication but does not attach to an arbitrary running Codex UI or terminal.
- Narrant currently starts Babysit turns with `gpt-5.6-luna` and `low` reasoning effort
  and does not expose model details in the Babysit Mode UI. User-facing model and
  reasoning configuration may replace this fixed choice later.
- A future Claude Code adapter may launch the installed Claude Code runtime and fulfill
  the same contract.
- A future Built-in Agent Provider is tracked separately. Rig is the initial candidate,
  but it is not part of the first vertical slice.
- Installed Codex reuses the reviewer's Codex home, authentication, and configuration.
  User MCP servers, plugins, hooks, skills, personal instructions, notifications, and
  history may participate normally. Each Babysit thread still uses a Narrant-owned
  read-only workspace permission profile with network access and approval escalation
  denied; inherited configuration cannot weaken that operation boundary.

### Runtime and Cache Decisions

- The Agent runs only after Workspace Trust and Review Workspace activation succeed.
- Workspace Trust is the only repository-level authorization gate for Babysit analysis;
  Babysit Mode does not add a separate code-upload notice or confirmation flow.
- The provider's Agent tools may read only the current Review Workspace. They cannot
  write files or access the network; the App Server's model-service transport remains
  available for Codex inference.
- One Narrant process performs no Codex discovery or I/O until the reviewer first
  enables Babysit Mode for an eligible File Change. It then discovers and lazily starts
  one Codex App Server child process. Every File Change analysis uses a fresh Codex
  thread, and all repair turns stay in that thread.
- Discovery first executes the bare `codex` command from the Narrant process PATH. It
  then tries a previously selected executable, bundled desktop locations, other known
  install locations, and finally offers a native executable picker. A selected path is
  a durable fallback setting, is validated with `codex --version`, and is never sent
  to the Agent or included in diagnostics. Narrant does not invoke a login shell.
- App Server compatibility is capability-based. After `initialize`, Narrant requires
  an authenticated account state plus the stable structured-output, restricted
  read-only sandbox, turn interruption, and thread-deletion behavior used by Babysit
  Mode. A candidate that cannot satisfy these capabilities does not shadow later
  executable candidates. Missing capabilities across all candidates disable the mode
  and request a Codex upgrade; otherwise the installed version is accepted. API-key,
  managed ChatGPT, and other App Server-supported account types are valid. The UI does
  not display a minimum tested version, and the protocol client ignores unknown
  notifications.
- Completed, failed, and cancelled analysis threads are permanently deleted from Codex
  history. Narrant marks its threads so the next lazy App Server startup can remove only
  Narrant-owned threads left behind by an abnormal exit.
- Switching File Changes, disabling Babysit Mode, refreshing the Review Snapshot, or
  closing the Review interrupts and deletes an incomplete thread. Request identity
  prevents late results from appearing on another File Change.
- Narrant terminates its App Server child process during application exit.
- A completed result is cached only in backend memory. The cache identity includes the
  Review Snapshot, File Change path, provider, App Server process generation, and
  analysis-contract version.
- Disabling Babysit Mode after completion hides the Line Explanations but retains the
  completed in-memory result. Re-enabling the mode displays that result immediately.
  Disabling during a run cancels the thread and retains no partial result.
- Incomplete results are never cached. Closing the Review or exiting the application
  discards its cached explanations. No SQLite persistence is introduced.
- One File Change run, including all repair turns, has a ten-minute total deadline.
  Timeout interrupts and deletes the thread, produces a visible retryable failure, and
  never caches partial output.

### File Eligibility

- The first vertical slice uses an explicit code-language allowlist and supports source,
  test, and script files without adding a product-level file-size or line-count limit.
  Provider or model output limits fail the whole run visibly; Narrant never presents a
  silently truncated set of Line Explanations.
- The initial allowlist covers TypeScript and TSX, JavaScript and JSX, Rust, Python, Go,
  Java, C, and C++. Extensionless and special-name scripts are eligible only when their
  detected language is one of these entries.
- Lock files, generated files, minified files, vendored code, snapshots, documentation,
  pure data files, binaries, symlinks, and submodules are skipped with a visible reason.
  The Babysit Mode control is disabled while the selected File Change is ineligible.

### Presentation Decisions

- Line Explanations appear in Monaco view zones immediately above their target lines;
  they are visually distinct from source comments.
- The selected File Change header owns the Babysit Mode toggle and, while enabled, the
  changed-lines-versus-all-lines filter. Narrow presentation reduces these controls to
  accessible icons with Tooltips. Ineligible File Changes disable the mode control and
  expose the specific reason in a Tooltip.
- Babysit Mode and its changed-lines-versus-all-lines filter remain active while the
  reviewer navigates among File Changes in the current Review Reader. They reset when
  the reviewer leaves that Review or exits Narrant and are not persisted as preferences.
- Results appear only after the whole File Change response passes validation. The
  original Diff remains readable while analysis runs or fails.
- Long explanations are visually truncated and can be expanded. Side-by-side Diff
  presentation preserves before-and-after alignment when explanation heights differ.
- `generating`, `repairing`, and `failed` state appears in a sticky footer inside the
  Diff panel. Generating and repairing provide Cancel; failure provides Retry and Copy
  Diagnostics. Completed and cancelled runs leave no footer, and file-scoped Agent
  state does not use a global Toast.
- If Codex is missing, incompatible, signed out, or fails to initialize, Babysit Mode
  shows a specific recovery action rather than starting analysis. Recovery may point to
  installation or upgrade instructions, `codex login`, or copyable redacted diagnostics
  as appropriate. The UI does not display a minimum tested Codex version.

### Diagnostics

- Babysit diagnostics may record only irreversible short hashes of Review, Snapshot,
  File Change, and Agent thread identities; Codex and protocol versions; run phases and
  durations; invalid-target counts; redacted error categories; process exit codes; and
  protocol error codes.
- Diagnostics never record source text, prompts, Line Explanations, full paths,
  authentication tokens, or raw App Server stdout and stderr.

## Testing Decisions

- Default CI never requires a Codex installation, user login, or paid model request.
- A controlled JSONL stdio App Server fixture covers initialization, complete output,
  three-round repair, malformed output, timeout, cancellation, process failure, late
  results, and thread deletion.
- Provider contract tests are reusable by future Installed Coding Agent adapters.
- A real installed Codex is exercised only by an explicit local smoke test.
- The repeatable local smoke procedure is documented in
  [babysit-smoke.md](../agents/babysit-smoke.md); it never records source or raw
  App Server output.
- React behavior tests and the Preview Harness cover generating, repairing, failure,
  changed-only and all-line filtering, single- and side-by-side Diff, narrow layout,
  line-above view zones, the sticky Diff footer, and before-and-after alignment.

## Out of Scope

- Persisting Line Explanations in SQLite or reusing them across Review Snapshots.
- Analyzing every File Change in a Review Snapshot in the background.
- Semantic explanation groups or a second Agent that checks coverage.
- AI Findings, code-quality judgments, suggested edits, or GitHub Review comments.
- Implementing the Claude Code or Built-in Agent Providers in the first vertical slice.

## Architecture Decisions

Provider separation and the provider-independent contract are recorded in
[ADR-0021](../adr/0021-separate-babysit-agent-providers-behind-one-contract.md).
Codex environment reuse and the set-aside isolation design are recorded in
[ADR-0022](../adr/0022-reuse-the-reviewers-codex-environment.md).

# Code Structure View validation

Status: implementation and A1–A18 verification complete within the accepted
static-analysis scope. Final implementation revision: `92c124b`. Tests, browser
inspection, fixed-revision measurements and independent real-source checks are
recorded below, with unmeasured platform/performance boundaries stated explicitly.

The isolated branch is `codex/ast-structure-review`, starting at
`da4f583089350b6fe8632fb83dcc9d7558e77286`. Specification 0018 and ADR-0030 are
accepted. Implementation follows the user-selected `implement` workflow. No
manual quality-gate is run.

## Environment

- Apple M2, Mac14,2, 8 cores, 8 GB; macOS 26.6.2.
- Node 24.18.0; Rust 1.97.1 (`8bab26f4f`, 2026-07-14).
- Rust debug test profile, incremental retained, separate worktree target;
  `scripts/cargo-agent.sh` supplies the repository lock and sccache, with two jobs.
- Heavy frontend and Rust checks are serialized. Interrupted or timed-out checks
  are never counted as passes.

## Executed automated checks

| Boundary | Actual result |
| --- | --- |
| Static parser provider | Eight production-module tests passed normally and with network denied; four linked grammar symbols verified. Included again in the complete application suite. |
| Rust production and tests | Full `check --lib` and `check --tests` passed before behavioral execution. |
| Structure analysis | Initial actual run: 132 passed, one failed, one benchmark ignored. The failure exposed duplicate trait-implementation occurrences; removing the redundant synthetic occurrence and asserting the exact native token range made all 27 relationship tests pass. |
| Fixed source access | Initial run: ten passed, one failed because a binary fixture incorrectly expected eleven instead of twelve bytes. The test now derives the exact expected byte count from its input; all eleven tests passed. |
| Backend integration | Input/process tests (six), structure lifecycle tests (seven) and Tauri structure command tests (two) passed using the real application test binary under the repository lock. |
| Complete Rust suite | The initial integrated suite passed 649 tests. After the real-snapshot glob correction, the final suite passed **665 tests, zero failed, one performance benchmark ignored**, in 72.20 seconds; preparation took 1.45 seconds. Binary and documentation targets also passed (zero tests). |
| Real-snapshot Rust glob regression | The initial public-engine test reproduced one bound implementation instead of four. Fifteen index regressions cover parent globs, lexical precedence, ambiguity, incomplete sources, logical module paths and visibility. The corrected public-engine regression verifies four old / five new groups and every member's owner, group, side and source; all are included in the final suite. |
| Frontend before real-browser fixes | Full TypeScript, ESLint, production build and 371 tests in 47 files passed. |
| Frontend after real-browser fixes | Full TypeScript and ESLint passed; nine affected files / 59 tests passed in 17.26 seconds. The renewed full suite passed **375 tests in 47 files**, in 58.87 seconds. The final production build also passed. |

The complete Rust command was:

```sh
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml -j 2 -- --test-threads=2
```

Rust emitted unused-import/internal-evidence warnings; no test failure remained.
The additional final Rust run was warranted by changed shared name-resolution
semantics. Frontend code and public transport types were unchanged by that fix,
so the existing final frontend build, 375-test suite and browser evidence remain
applicable. The engine regression's fixture selector distinguishes same-named
modules from methods; an added implementation header may legitimately mark the
aggregate type changed while its native struct signature remains identical.
Generated Tauri schema ordering changes were checked for JSON semantic equality
and restored. No production schema change was discarded.

The final production build transformed 2,203 modules in 3 minutes 56 seconds
(total command wall time 243.80 seconds), with no error. Main JS was 5,747.67 kB
(1,471.36 kB gzip) and CSS 298.43 kB (48.63 kB gzip). Vite emitted its large-chunk
advisory; no baseline bundle comparison was measured. An earlier build took
24 minutes 23 seconds under heavier memory pressure; the final result above is
the validated build after the browser fixes.

Parser details and provider boundaries are recorded in
[code-structure-parser.md](code-structure-parser.md). Exact command logs and
per-task reports are retained in the ignored execution ledger.

## Real browser evidence

The actual ReviewReader is rendered through the reusable Vite Preview Harness
using representative in-memory adapters, without changing real review data.
CUA operated the in-app browser. Intermittent browser-control timeouts were not
counted as successful interactions. Temporary viewport overrides were reset
before the Rust validation window.

Actual evidence already obtained:

- At 1440 × 1000, the moved declaration's destination file scrolls to and
  highlights line 121. Selecting the old location loads the old file's own full
  pair; single-column mode highlights its deleted line 121. File Changes keeps
  its independent destination-file selection.
- At a measured 720 × 900 in dark mode, long-source Peek automatically uses
  single-column mode and line 121 remains visible. Document width was 706 px,
  without horizontal overflow; the Harness header produced vertical scrolling.
- Entering Peek from the actual graph button and pressing Escape restores focus
  to that same button. Switching to File Changes reads `notes.md`; returning to
  the graph retains the Peek position. Refresh changes Snapshot identity from
  1 to 2 and clears obsolete graph/Peek state before showing new progress.
- During analysis, ordinary File Changes remains readable. Cancel shows a
  cancelled state; retry returns to progress and can complete.
- Added/deleted files explicitly identify the absent side. A readable zero-byte
  side has no absence/unavailable warning. An unavailable Git object is explained
  separately while the readable side remains a full source editor; retry works.
- A type's second implementation source opens its real
  `persist_snapshot_state.rs` full file rather than a synthetic combined file.

Real rendering found and drove these fixes:

1. The installed Monaco API exposes `ScrollType` under `editor`. Tests now import
   the actual Monaco API rather than fabricating that enum. The regression first
   reproduced the browser error, then passed with the corrected three accesses.
2. Monaco's React wrapper disposed still-attached Diff models. Cleanup now saves
   view state, detaches models, releases them, and leaves editor disposal to the
   wrapper. A cleanup-order regression failed before the fix. A fresh page load,
   repeated navigation, view switching and Peek close produced no new error.
3. A fixed 420 px narrow canvas was clipped by Peek. The graph now uses its actual
   available height, preserves manual camera movement and exposes compact narrow
   controls and a collapsible details area. At 720 × 900, the final canvas was 167.55 px high, compared with 88 px before compact controls. Details remained accessible. After manual zoom, changing the splitter from 58 to 53 retained the exact camera transform.
4. Long source paths made different menu items indistinguishable. Items now lead
   with filename/line/side and wrap the full path separately. At 720 × 900, the popup measured 625 px client and scroll width, with full paths wrapping and no horizontal overflow. Six declaration/implementation sources were distinguishable and selecting the second implementation loaded its real file. Four call-site choices preserved side and line; old line 6 highlighted the actual deleted call, while the target-definition action opened the separate declaration file at line 4.

The final graph checks also verified default controls (selected-node one-hop
calls; inheritance/implementation/external on; type/import/candidate off), dotted
candidate lines, independent relation toggles and external hiding. Relation-only
changes remain discoverable when their type line is hidden, without altering
counts. Native members reappear on expansion and folding preserves totals.

Searching the 180-node batch sample reported all 180 matches. Two expansions
changed visible counts from 61/181 to 121/181 to 181/181, and the final item
navigated to `decl:bulk:179`. A missing query showed an explicit empty result.
CUA action-plus-observation round trips were 586 ms for search and 2,364 ms for
the first expansion; these include automation overhead and are **not isolated
render timings or Narrant backend performance**.

Partial coverage exposes reasons, cancellation keeps its incomplete state,
errors can retry and empty graphs have an explicit message. Rename retains both
actual paths and its explicit File Changes action opens the new path. Unchanged
source has no fabricated File Changes action. Light and dark screenshots and
computed styles confirmed distinguishable change colours with textual labels.
The final round left no new runtime errors; the retained console only contained
historical hot-reload warnings. Viewport overrides were reset and the temporary
preview server stopped after verification.

## Acceptance mapping

| Criterion | Evidence / result |
| --- | --- |
| A1 | Fixed-source, lifecycle and IPC tests passed; actual view switching, cancel/retry and Snapshot refresh verified. |
| A2 | Native TS/JS/Rust extraction tests passed; real graph showed native struct/impl/member hierarchy. |
| A3 | Whole-tree source/index/engine tests passed; graph search and expansion verified. Both fixed Narrant trees were discovered, with 813 complete supported file-side extractions. |
| A4 | Resolved/candidate/unresolved, binding/shadow and alias tests passed; relation controls and reasons rendered. |
| A5 | Conservative matching and cross-file owner tests passed; real old/new source navigation verified. |
| A6 | Fingerprint tests covering formatting and semantic token changes passed. |
| A7 | Documentation-only public-engine focus/Peek test passed. |
| A8 | Rust module/impl aggregation tests passed; real Narrant ReviewLifecycle has four / five implementation groups with 39 / 44 callable members, retaining exact owners, native kinds and separate files. |
| A9 | Named nested functions, callbacks, lexical mutation and provenance tests passed; callback provenance is visible in relation cards. |
| A10 | Multi-label and parent/child counting tests passed; graph folding preserved totals and labels use distinct colours. |
| A11 | Config-only relationship changes and unknown-state tests passed; hidden lines retain discoverable change entries and counts. |
| A12 | Real graph/source/occurrence/definition navigation, independent File Changes, resize and Escape focus verified. |
| A13 | Syntax/resource/object/cancellation tests passed; unavailable, missing, empty and partial states verified in the browser. |
| A14 | Static offline parser tests and backend lifecycle tests passed; analysis progress/cancel does not block actual file reading. |
| A15 | Repository relationship tests passed; independent default-off type/import toggles, candidate display and external hiding verified. |
| A16 | Real rename, cross-file move, unchanged source and multiple impl files verified, backed by source pairing tests. |
| A17 | UTF-16/CRLF/readiness tests passed; actual old deletion highlight, missing/empty/unavailable sides and narrow single-column reading verified. |
| A18 | 180-match batch search/expansion verified; fixed Narrant source counts, two complete analysis timings, peak process memory and cancellation latency measured below. |

## Independent reviews

Standards review's three findings were corrected and rechecked: use shadcn
buttons, retain enum relationship keys and share language-extension detection.

Spec review's original three findings and two follow-ups were corrected and
rechecked: wrapper signatures, lexical rebinding, named function expressions,
`this` member writes and undeclared loop assignment targets. A later bounded
review of editor cleanup, camera state and source-menu changes found no new
concrete specification issue. Reviews were static; executed tests and browser
observations are separate evidence.

The subsequent real Narrant audit found that `use super::*` implementations were
not assigned to `ReviewLifecycle`. The index now resolves these imports through
the actual same-side module graph while preserving ambiguity, lexical shadowing
and incomplete evidence. A final review caught an inaccessible intermediate
module path; the correction checks each logical module declaration before using
its contents. Restricted visibility and unproven re-exports remain conservative.
That bounded re-review passed, and all final regressions were executed.

## Fixed Narrant scale and performance

The final measurement uses the corrected implementation at `92c124b`. The earlier
measurement at `e040af3`, which exposed the parent-glob ownership gap, is retained
separately in the ignored execution ledger and is not mixed into these results.

Independent Git blob counting reproduced the accepted baseline at
`da4f583089350b6fe8632fb83dcc9d7558e77286`:

| Language | Files | Bytes | Physical lines |
| --- | ---: | ---: | ---: |
| TypeScript | 152 | 1,902,555 | 46,305 |
| JavaScript | 46 | 378,997 | 8,720 |
| Rust | 177 | 1,657,098 | 45,394 |
| Total | 375 | 3,938,650 | 100,419 |

The generated icon manifest is included: 934,138 bytes and 25,527 physical lines.
The implementation after revision is `92c124beef39872898b4f27845e956d66f08b1ce`.
Its independently counted source scope is:

| Language | Files | Bytes | Physical lines |
| --- | ---: | ---: | ---: |
| TypeScript | 179 | 2,135,508 | 49,540 |
| JavaScript | 46 | 378,997 | 8,720 |
| Rust | 213 | 2,315,485 | 63,152 |
| Total | 438 | 4,829,990 | 121,412 |

Counting reads tracked blobs from those exact revisions with replacement refs
disabled. It does not use checkout contents or the analyzer's counters. The
generated manifest is unchanged and included on both sides.

The opt-in `measure_fixed_narrant_snapshot` test passed separately from the full
suite. The already-compiled application test binary ran under the same repository
lock, with one test thread and no concurrent compiler, frontend build or preview
server. The measured build is the debug test profile.

| Metric | First analysis | Second analysis |
| --- | ---: | ---: |
| First usable result | 34,025 ms | 32,688 ms |
| Full analysis | 34,081 ms | 32,760 ms |
| Extraction begins | 181 ms | 210 ms |
| Matching begins | 27,274 ms | 25,678 ms |
| Relationship analysis begins | 28,966 ms | 27,647 ms |
| Result nodes / implementation groups / relationships | 9,375 / 350 / 17,078 | 9,375 / 350 / 17,078 |

Times for phase beginnings are offsets from each analysis start. The first
analysis initializes this process's parser rules; it is **not a cold filesystem
measurement**, because independent Git counting had already read both versions.
The second analysis reuses thread-local parser rules and OS caches, with no
analysis-result cache. Optional full-result JSON export is excluded from the
per-analysis elapsed time. Progress arrives during extraction, but the first
usable graph checkpoint currently arrives near completion. These two debug
measurements do not establish release-build latency or a statistical distribution.

The complete benchmark process, including both analyses, optional JSON export
and cancellation, took 74.99 seconds. macOS `/usr/bin/time -l` recorded maximum
resident set size **372,490,240 bytes (355.23 MiB)** and peak memory footprint
**557,728,776 bytes (531.89 MiB)**, with zero swaps. These are whole-process peaks,
not isolated memory for one analysis, and exclude compilation.

Cancellation requested immediately after the first extracted file reached its
terminal state in **17.348 ms** (993 ms from analysis start). The result remained
cancelled and incomplete, with 36 incomplete declarations and no false added or
deleted declaration changes.

Both rounds discovered both trees completely and recorded the same coverage:
813 complete supported file-side extractions, plus 12 partial entries for changed
unsupported files (three before, nine after). Relationship coverage was complete
for 75 file-sides, partial for 738 and not started for the 12 unsupported entries.
The completed analysis therefore correctly retained `summary.complete: false`:
1,738 changed declarations, nine incomplete declarations, 1,780 relationship
changes and 53,536 unresolved occurrences. Completing an analysis does not make
its static relationship evidence complete.

Diagnostic occurrence totals were 51,745 `dynamic-target`, 1,179
`configuration-unavailable`, 588 `ambiguous-target`, 14 `unreliable-scope` and 12
`unsupported-language`. These are occurrences, not omitted source-file counts.
Unknown facts retain their reasons rather than being converted into deletions.

## Independent real-source audit

The full result was independently checked against both fixed Git trees and blobs.
The source catalog, coverage paths and supported Git paths match exactly: 375
before and 438 after, with no missing or extra path and complete extraction for
every supported file-side, including the generated icon manifest.

`ReviewLifecycle` now has the exact implementation groups and callable members
present in the original files:

| File under `src-tauri/src/review/lifecycle/` | Before members | After members |
| --- | ---: | ---: |
| `mod.rs` | 15 | 15 |
| `presentation.rs` | 10 | 10 |
| `refresh.rs` | 9 | 9 |
| `workspace.rs` | 5 | 5 |
| `structure.rs` | absent | 5 |
| Total | 39 across four groups | 44 across five groups |

The native breakdown is 34 methods plus five associated functions before, and
39 methods plus five associated functions after. Each group's owner, each member's
parent/group/source/side, and implementation/member ranges match the fixed source;
per-file member-name sets match exactly. No member was duplicated or reassigned to
a synthetic combined file. Adding an implementation header can mark the aggregate
type changed independently of its unchanged native struct signature.

`branchName` retains ten resolved call edges with 20 occurrences on each side,
including 18 cross-file occurrences through explicit imports. Its callers include
`home-page.tsx`, `branch-review-selector.tsx`, `review-overview.tsx` and
`repository-overview.tsx`; callback provenance remains distinct from direct
execution. `Toaster` in `src/components/ui/sonner.tsx` remains a TypeScript
`function` category with the native `arrow-function` name. JSX usage is not
automatically claimed as a resolved ordinary call.

The parent-glob correction reduced configuration-unavailable occurrences from
2,861 to 1,179, while retaining uncertain facts. Residual reasons include module
contexts introduced through `include!`, Rust documentation outside the Cargo
module graph, the bounded built-in prelude table and non-source resource imports.
These limits do not remove files from extraction coverage or imply nonexistent
declarations.

## Presentation and resource settings

The graph renders batches of 60 nodes and relation evidence in batches of 20;
search considers the complete result, with explicit visible/pending counts and
further expansion. Git source reading batches target 8 MiB or 512 objects and
retain the existing 16 MiB output limit. An oversized file has explicit resource
coverage and retry/source availability semantics; it is not treated as absent.
No Narrant supported source was excluded to fit these settings. They are current
bounded defaults, not claimed optimal performance settings.

Compact measurements, independent counts, memory readings and cache definitions
are committed in [code-structure-benchmark.json](code-structure-benchmark.json).
The larger raw result and exact process log remain in the ignored execution
ledger. The benchmark is reproducible through the existing ignored test:

```sh
NARRANT_STRUCTURE_BENCH_REPOSITORY="$PWD" \
NARRANT_STRUCTURE_BENCH_BEFORE=da4f583089350b6fe8632fb83dcc9d7558e77286 \
NARRANT_STRUCTURE_BENCH_AFTER=92c124beef39872898b4f27845e956d66f08b1ce \
NARRANT_STRUCTURE_BENCH_OUTPUT=/tmp/narrant-structure-measurement.json \
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml -j 2 \
  review::structure::engine::tests::measure_fixed_narrant_snapshot \
  -- --exact --ignored --nocapture --test-threads=1
```

That command may compile first; its total command time and memory are not the
isolated process measurements reported above. Native Tauri end-to-end render/IPC
timing, release-profile analysis, and other operating systems were not measured.
Browser acceptance used the real reader and editors with representative Preview
Harness data; the full Narrant engine measurement was a separate real-snapshot
run.

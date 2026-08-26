# Issue 141B: Shared Custom runtime and provider integration

## Goal

Build the reusable Custom runtime on the 141A protocol seam and replace the placeholder
`CustomProvider` behavior behind `BabysitAgentProvider` and the capability registry.

## Public test seams

- `BabysitAgentProvider::{generate, repair, abandon}` using deterministic adapter/HTTP fixtures.
- Highest practical `BabysitApplication` contract seam, including completed-cache behavior.
- Frozen connection snapshot/cache identity excluding credentials while including credential
  generation and all non-secret connection fields.

## Requirements

- Freeze protocol, exact base URL, optional key, model, effort, and credential generation at
  analysis start; connection changes must not mutate in-flight work.
- Reuse one in-memory conversation across initial generation and at most three repairs. Each round
  permits at most 16 model turns. Do not duplicate application-owned validation, repair merging,
  or ten-minute whole-run deadline.
- Bind repository tools to the request's canonical Review Workspace and cancellation signal.
- Cancellation drops the active HTTP future, prevents later local tool dispatch, rejects late final
  output, abandons conversation state, and cannot populate completed cache.
- Cover repository tool loop, complete output, malformed output repaired in the same conversation,
  exhausted repairs, 16-turn exhaustion, cancellation before/during model and tool work, late output
  rejection, redacted failures, and frozen completed-cache isolation.
- Preserve the narrow adapter seam for Issue 142 and do not implement Anthropic wire shapes.
- Keep production type/function/method comments readable and in Chinese. Put tests in the module's
  separate `tests/` directory via `#[path]`.
- TDD one vertical behavior at a time. Run only focused tests through `scripts/cargo-agent.sh` and
  never overlap Cargo invocations.

## Acceptance

- Placeholder setup failure is gone for configured OpenAI Custom connections.
- Existing installed-provider behavior remains unchanged.
- Focused Rust tests pass; the drone will run the single full Rust suite after merge.

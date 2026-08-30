# Issue 141: OpenAI-compatible Custom provider

## Objective

Implement the reusable Custom provider runtime and its OpenAI Chat Completions protocol
adapter behind the existing `BabysitAgentProvider` seam. The runtime owns conversation,
repair, cancellation, tool dispatch, output validation handoff, and frozen connection
state; protocol-specific request construction remains in the OpenAI adapter.

## Ownership boundary

- This slice owns the shared Custom runtime under `babysit/custom/`, the final
  schema-bearing Line Explanation tool, and the OpenAI adapter.
- It may replace the temporary `CustomProvider` placeholder introduced by Issue 139.
- It must expose a narrow protocol-adapter seam that the Anthropic slice can implement
  without duplicating conversation, repair, cancellation, or tool-loop policy.
- It must not implement Anthropic request or response shapes.
- Existing provider-independent validation and application repair orchestration remain
  authoritative; do not fork those rules inside the adapter.

## Required behavior

- Freeze protocol, base URL, optional credential, model, effort, and credential
  generation at generation start. Never put the credential in cache identity, logs, or
  diagnostics.
- Construct the Rig OpenAI Chat Completions client from the exact saved base URL, key,
  and model. Empty key remains empty and URL behavior is Rig's default.
- Map effort to the wire `reasoning_effort` value `low`, `medium`, or `high`.
- Register exactly Read, Glob, Grep, and one internal schema-bearing final-output tool.
  The final tool uses Rig `OutputMode::Tool`; no shell, write, edit, Web, MCP, native
  structured-output format, or additional model tool is available.
- Reuse one in-memory conversation for initial generation and up to three repairs.
  Each generation or repair round permits at most 16 model turns. Preserve the existing
  ten-minute whole-run deadline owned by the application layer.
- Cancellation drops the active HTTP future, prevents later local tool dispatch,
  rejects late final output, abandons conversation state, and cannot populate the
  completed cache.
- Repository tools bind to the request's canonical Review Workspace and cancellation
  signal. The final tool returns the provider-neutral compact before/after payload for
  existing validation and repair merging.
- Provider and transport errors are redacted. They may carry protocol, phase, status
  category, and an irreversible endpoint-origin hash, never URL, key, bodies, prompts,
  source, explanations, or repository tool output.

## Verification

- Use deterministic local HTTP fixtures; no paid or live endpoint.
- Assert request path, model, optional authentication behavior, `reasoning_effort`,
  exact registered tool names, and final-tool schema.
- Cover a repository-tool loop, complete output, malformed final output followed by
  same-conversation repair, three exhausted repairs, 16-turn exhaustion, cancellation
  before/during model and tool work, late output rejection, and redacted failures.
- Run the reusable provider contract through the highest practical Babysit application
  seam and prove completed-cache isolation by frozen non-secret connection identity.


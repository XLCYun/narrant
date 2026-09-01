# Issue 142: Anthropic-compatible Custom provider

## Objective

Implement the Anthropic Messages protocol adapter for the shared Custom provider
runtime, using the same repository tools, final-output contract, conversation, repair,
cancellation, and cache semantics as the OpenAI-compatible path.

## Ownership boundary

- This slice owns Anthropic-specific client construction, request configuration,
  response/tool conversion, and deterministic transport fixtures under a dedicated
  module such as `babysit/custom/anthropic.rs`.
- Keep protocol-neutral runtime policy out of the Anthropic adapter. If the shared
  runtime is not yet present in this worktree, define the smallest compileable adapter
  contract locally and document the integration seam; Issue 143 will reconcile it with
  the OpenAI slice.
- Avoid editing the temporary provider registry or settings/UI except where a narrow
  compile seam is unavoidable.
- Do not implement OpenAI request or response shapes.

## Required behavior

- Construct the Rig Anthropic Messages client from the exact saved base URL, optional
  key, and user-entered model. Empty key remains empty and URL behavior is Rig's default.
- Every generation and repair request enables adaptive thinking and maps effort to
  `output_config.effort` as `low`, `medium`, or `high`.
- Use the internal schema-bearing Line Explanation tool through Rig
  `OutputMode::Tool`. Do not also send Anthropic native `output_config.format`, because
  it competes with tool output.
- Expose only Read, Glob, Grep, and the final-output tool to the model.
- Let Rig alone own each model-and-tool loop. Preserve its 16-turn-per-generation-or-
  repair limit, one conversation across initial plus up to three repairs, request-bound
  workspace/cancellation, late-output rejection, completed-only caching, and frozen
  non-secret connection identity; Narrant must not wrap Rig in another turn or tool loop.
- Send `max_tokens: 65536` on every model turn. Do not split targets into batches,
  silently reduce or retry the budget, or use a model-name fallback. Rejecting the budget
  is an explicit provider failure; reaching it is a distinct output-limit failure and
  does not enter malformed-output repair.
- Authentication and endpoint construction are delegated to Rig. Diagnostics and
  errors obey the same redaction rules as the OpenAI adapter.

## Verification

- Use deterministic local HTTP fixtures; no paid or live endpoint.
- Assert Messages endpoint/request shape, model, optional authentication behavior,
  adaptive thinking, effort, tool declarations, and absence of native output format.
- Cover tool-use/tool-result loops, final tool output, malformed output and
  same-conversation repair, three exhausted repairs, Rig-owned 16-turn exhaustion, the
  fixed output budget, rejected budgets, output-limit failure without repair,
  cancellation at model and tool boundaries, late responses, and redacted HTTP/protocol
  errors.
- Reuse the protocol-neutral provider contract wherever the shared runtime seam is
  available; otherwise deliver adapter-level contract tests that Issue 143 can plug
  into that harness without changing Anthropic behavior.

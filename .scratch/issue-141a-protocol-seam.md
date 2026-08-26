# Issue 141A: Custom protocol seam and OpenAI adapter

## Goal

Replace the placeholder file layout with a `babysit/custom/` module that defines the narrow,
protocol-neutral adapter boundary shared by OpenAI now and Anthropic in Issue 142. Implement the
OpenAI Chat Completions adapter and the schema-bearing final Line Explanation tool.

## Public test seams

- The protocol adapter interface: frozen non-secret/secret connection inputs, model round result,
  tool calls, redacted transport failures, and request-turn limit behavior.
- Rig tool metadata for exactly `Read`, `Glob`, `Grep`, and the single final-output tool.
- Deterministic local HTTP fixture assertions for path, model, optional auth, reasoning effort,
  tool names, and final-tool schema.

## Requirements

- Use Rig 0.42 APIs; no live or paid endpoint.
- Pass the exact saved base URL, key (including empty), and model to the OpenAI client; allow Rig
  to own URL/path and standard auth behavior.
- Map low/medium/high to `reasoning_effort` and use `OutputMode::Tool` for the final result.
- The adapter seam must not expose OpenAI request/response shapes to the shared runtime and must be
  implementable by Issue 142 without duplicating conversation/tool-loop policy.
- Only repository Read/Glob/Grep plus one internal final tool may be registered.
- Provider/transport errors may expose protocol, phase, status category, and irreversible origin
  hash only. Never expose URL, key, bodies, prompts, source, explanations, or tool output.
- Keep production type/function/method comments readable and in Chinese. Put tests under
  `src-tauri/src/babysit/custom/tests/`, connected with `#[path]`.
- TDD one vertical behavior at a time. Run only focused tests through `scripts/cargo-agent.sh` and
  never overlap Cargo invocations.

## Out of scope

- Shared generate/repair orchestration and provider registry wiring (141B).
- Anthropic request/response implementation.


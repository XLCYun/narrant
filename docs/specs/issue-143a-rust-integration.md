# Issue 143A: Custom Rust integration contract

## Objective

Close the remaining backend integration seams for the Custom Babysit provider and prove
both supported protocols through deterministic application-level fixtures.

## Scope

- Exercise OpenAI Chat Completions and Anthropic Messages through provider registry
  selection and the complete File Change analysis lifecycle.
- Prove output completion, repository tool calls, repair in the same conversation,
  repair exhaustion, cancellation and late responses, completed-only caching,
  connection identity isolation, and frozen in-flight connection snapshots.
- Assert protocol effort mapping, optional authentication, exact Read/Glob/Grep and
  `final_result` tools, schema output, and redacted diagnostics.
- Ensure the Anthropic adapter is selected through `CustomCapabilityBundle` and remove
  obsolete backend slice seams, dead paths, unjustified suppressions, and placeholders.
- Keep all protocol wire details within `src-tauri/src/babysit/custom/`.

## Constraints

- Use deterministic local fixtures only; never make a paid or external model request.
- API keys, full URLs, prompts, source, responses, explanations, and repository output
  must not appear in diagnostics or test failure messages.
- Add Chinese comments for methods, structs, and classes. Keep Rust tests in module
  `tests/` directories.
- Do not run the Issue-wide full Rust suite. The drone owns the single final full run.
- Any Cargo command must use `scripts/cargo-agent.sh`; do not run duplicate commands,
  `cargo clean`, or manually trigger quality-gate.

## Verification

- Run only focused Rust checks/tests required to develop the slice, after drone approval.
- Report exact commands, counts, residual risks, commit, and changed files.


# Issue 143: Custom provider integration and release verification

## Objective

Finish Custom as one production Babysit provider by validating the already merged
settings, repository tools, shared runtime, OpenAI adapter, and Anthropic adapter through
the application and Review lifecycle. Remove temporary slice seams and document an
opt-in smoke procedure without making a live model request in CI.

## Integration requirements

- Both explicit protocols must be constructible through `CustomCapabilityBundle` from
  the complete saved connection and must run through the same `BabysitAgentProvider`,
  validation, repair, progress, cancellation, and completed-cache paths.
- The Anthropic adapter must consume the shared `CustomProtocolAdapter` contract rather
  than remain an adapter-only dead-code path. OpenAI and Anthropic must expose no
  protocol wire types outside `babysit/custom/`.
- A run freezes protocol, URL, model, effort, key, and credential generation. Saving a
  changed connection invalidates completed Custom cache entries, while a request already
  in flight retains its original snapshot.
- Empty credentials remain valid for both protocols. Standard authentication headers
  are delegated to Rig, but an empty credential must not create a misleading non-empty
  authentication value.
- Both adapters expose exactly Read, Glob, Grep, and `final_result`; final output uses
  Rig Tool mode and returns to the provider-independent validation/repair contract.
- Rig is the sole model-and-tool agent-loop owner. Each generation or repair gives Rig at
  most 16 model turns with a 65,536-token output budget; the shared runtime owns no
  duplicate turn or repository-tool dispatch loop. The integration does not batch
  targets, reduce or retry a rejected budget, or repair output that reached the limit.
- Cancellation at HTTP, repository tool, repair, and late-response boundaries prevents
  subsequent local dispatch and completed-cache writes.
- Diagnostics are stable redacted values only: Custom identity, protocol, phase, and
  status/error category, with no endpoint fingerprint. Tests must prove URL, key,
  request/response body, prompt/source, explanation, and repository output do not occur
  in user-visible or debug error text.
- An explicitly enabled development-observability build exports Rig's OpenTelemetry
  GenAI spans, Narrant analysis-lifecycle spans, and structured logs over OTLP
  HTTP/protobuf. It defaults to Venator at `http://127.0.0.1:8362`, honors
  `OTEL_EXPORTER_OTLP_ENDPOINT`, keeps terminal logs, never blocks analysis when the
  dashboard is absent, uses bounded queues, and waits at most two seconds to flush on
  shutdown. It never captures prompts, source, explanations, tool arguments, or tool
  results.
- Remove obsolete `.scratch` slice specs, placeholder comments, unused alternate
  execution paths, and suppressions that are no longer justified. Keep intentional
  diagnostic fields only when a production or test consumer proves the seam.

## Automated verification

- Add a reusable application/provider contract that runs deterministic OpenAI and
  Anthropic HTTP fixtures through registry selection and the full File Change analysis
  path. It must cover complete output, repository tool loop, malformed output repair in
  the same conversation, repair exhaustion, cancellation/late response, completed-only
  caching, connection identity isolation, and frozen in-flight configuration.
- Assert final wire shapes for both protocols, effort mapping, optional authentication,
  exact tools, the fixed 65,536-token output budget, distinct rejected-budget and
  output-limit behavior, and schema output without a paid request.
- Capture development traces and logs with a deterministic OTLP fixture. Assert Rig
  GenAI spans, Narrant analysis correlation, the Venator default, the standard endpoint
  override, non-blocking receiver absence, bounded shutdown, and exclusion of prompts,
  source, explanations, tool arguments, and tool results.
- Run affected frontend tests, full frontend tests, lint, type/build checks, focused
  Rust tests, and exactly one final full Rust test for this Issue. Do not manually
  trigger `quality-gate`.
- Classify any unrelated pre-existing environment/flaky failure with a focused rerun or
  evidence; do not hide a Custom regression behind that classification.

## Visual verification

- Use `pnpm dev:preview` and the in-app browser.
- Inspect wide and narrow viewports for empty, configured, invalid, saving, failed,
  password-hidden, password-revealed, and unconfigured recovery states.
- Verify screenshots, accessible DOM, password input type/visibility action, error
  retention, relevant element dimensions, and absence of horizontal overflow. Restore
  the default viewport afterward.

## Documentation

- Add an opt-in smoke-test document for reviewer-supplied OpenAI-compatible and
  Anthropic-compatible endpoints.
- The procedure must never echo or record a key, full endpoint URL, prompts, source,
  responses, explanations, or tool results; it must be clearly excluded from CI.

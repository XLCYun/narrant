---
status: accepted
---

# Use Rig for the Custom Babysit provider

Narrant will add a Built-in Agent Provider displayed as Custom and implemented with
Rig. It owns one user-configured model-service connection using either the OpenAI Chat
Completions or Anthropic Messages protocol, with a base URL, model, reasoning-parameter
preference, and optional API key stored in the operating-system credential store for
release builds. Fresh
settings select Custom so that Narrant never consumes an installed coding agent's quota
until the reviewer explicitly chooses that provider; Custom remains unavailable until
configured and never silently falls back.

Rig receives the selected base URL and credential and owns the protocol request. Narrant
maps an explicit low/medium/high effort into the selected protocol's request parameters,
while Provider default omits those protocol-specific controls. It registers only bounded
Read, Glob, and Grep repository tools and uses Rig's schema-bearing Tool output mode for
the final Line Explanations. Tool output keeps the provider-independent validation and
three-round repair contract while avoiding incompatible native structured-output shapes
between the two supported protocols.

Rig is the only owner of the model-and-tool agent loop. Each initial generation or repair
run gives Rig at most 16 model turns and a 65,536-token output budget; Narrant does not
wrap that run in another turn loop, split one result into batches, silently reduce the
budget, or repair a response that ended because it reached the output limit. Narrant
retains only the cross-run responsibilities: the shared transcript, up to three repairs,
the ten-minute analysis deadline, validation, cancellation, and completed-result caching.

## Consequences

- The stable provider identity is `custom`; `rig` remains an implementation detail.
- The settings screen loads the complete Custom connection, including its credential,
  and submits protocol, URL, key, model, and effort together rather than patching fields.
  The credential may cross the in-memory Tauri boundary for this editing flow. Release
  builds persist it only in the operating-system credential store; Debug builds use a
  separate `custom-provider-credential.dev.json` beside `settings.json`, with owner-only
  permissions on Unix.
- Saving writes non-secret settings before updating the credential and restores the old
  settings if credential storage fails.
- Debug builds do not read the operating-system credential store by default. Setting
  `NARRANT_DEV_IMPORT_CUSTOM_PROVIDER_KEYCHAIN=true` opts into one migration read when
  the local development credential document does not yet exist; the migrated value or
  explicit empty sentinel prevents future reads.
- URL handling and redirects follow Rig's HTTP behavior without a Narrant-owned endpoint
  allowlist or redirect policy.
- The Rig dependency explicitly enables its Reqwest and Rustls features, and both
  protocol request guards construct a Rustls client. HTTPS support must not depend on
  Rig default features because this application disables them deliberately.
- A missing Custom connection produces an explicit settings recovery action and never
  selects Codex or Claude Code automatically.
- Provider default means Narrant does not override the service's reasoning behavior; it
  does not promise that the selected model disables reasoning.
- Narrant does not retry a failed request with different reasoning parameters. A service
  that rejects an explicit effort returns one redacted provider error for that request.
- A service that rejects the 65,536-token output budget fails explicitly. A response that
  reaches that limit produces a distinct output-limit failure instead of malformed-output
  repair.
- Custom diagnostics retain stable protocol, phase, status, and redacted provider-error
  categories but no endpoint fingerprint. URL, key, bodies, prompts, source, Line
  Explanations, and repository-tool content remain excluded.
- Debug builds install only a local `tracing-subscriber::fmt` terminal subscriber; Release
  builds install none. Narrant deliberately does not ship OpenTelemetry, OTLP export, a
  collector, or a dashboard integration because local performance diagnosis does not
  justify that dependency and lifecycle surface. Rig's native completion spans remain,
  with content telemetry disabled, while Narrant records correlated lifecycle, model,
  provider-request, and tool boundaries with explicit elapsed time. Development logs may
  contain the configured endpoint URL and model identifier, but never the API key, prompt,
  source, model body, explanation, tool arguments, or tool results.

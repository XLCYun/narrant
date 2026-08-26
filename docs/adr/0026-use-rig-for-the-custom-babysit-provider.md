---
status: accepted
---

# Use Rig for the Custom Babysit provider

Narrant will add a Built-in Agent Provider displayed as Custom and implemented with
Rig. It owns one user-configured model-service connection using either the OpenAI Chat
Completions or Anthropic Messages protocol, with a base URL, model, low/medium/high
effort, and optional API key stored in the operating-system credential store. Fresh
settings select Custom so that Narrant never consumes an installed coding agent's quota
until the reviewer explicitly chooses that provider; Custom remains unavailable until
configured and never silently falls back.

Rig receives the selected base URL and credential and owns the protocol request. Narrant
maps effort into the selected protocol's request parameters, registers only bounded
Read, Glob, and Grep repository tools, and uses Rig's schema-bearing Tool output mode for
the final Line Explanations. Tool output keeps the provider-independent validation and
three-round repair contract while avoiding incompatible native structured-output shapes
between the two supported protocols.

## Consequences

- The stable provider identity is `custom`; `rig` remains an implementation detail.
- The settings screen loads the complete Custom connection, including its credential,
  and submits protocol, URL, key, model, and effort together rather than patching fields.
  The credential may cross the in-memory Tauri boundary for this editing flow but is
  persisted only in the operating-system credential store.
- Saving writes non-secret settings before updating the credential and restores the old
  settings if credential storage fails.
- URL handling and redirects follow Rig's HTTP behavior without a Narrant-owned endpoint
  allowlist or redirect policy.
- A missing Custom connection produces an explicit settings recovery action and never
  selects Codex or Claude Code automatically.

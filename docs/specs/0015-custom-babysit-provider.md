# Custom Babysit Provider

## Problem Statement

Babysit Mode currently requires the reviewer to install and explicitly select Codex or
Claude Code. Reviewers who prefer a direct model-service connection cannot use their
own compatible endpoint, while silently selecting an installed provider would consume
that provider's quota without an explicit choice.

## Solution

Narrant adds a Built-in Agent Provider displayed as Custom. It uses Rig inside the
backend to call one user-configured OpenAI Chat Completions-compatible or Anthropic
Messages-compatible endpoint. The reviewer supplies a base URL, optional API key,
model identifier, and effort preference. Custom implements the existing whole-File-
Change Babysit contract, explores only the active Review Workspace through bounded
Narrant-owned Read, Glob, and Grep tools, validates a schema-bearing final tool result,
and keeps all analysis state in memory.

Fresh settings select Custom but do not make a network request. Babysit remains
unavailable until the reviewer configures Custom or explicitly chooses an Installed
Coding Agent Provider in Application Settings.

## User Stories

1. As a reviewer, I want to connect Babysit directly to my model service, so that I do
   not need an installed Coding Agent runtime.
2. As a reviewer, I want to choose an OpenAI-compatible or Anthropic-compatible
   protocol explicitly, so that Narrant never guesses from my URL.
3. As a reviewer, I want to provide my own base URL, so that I can use a public, local,
   private, or proxied compatible endpoint.
4. As a reviewer, I want the API key to be optional, so that unauthenticated local
   endpoints remain usable.
5. As a reviewer, I want to enter the model identifier myself, so that Custom does not
   depend on an optional model-listing API.
6. As a reviewer, I want low, medium, or high effort, so that I can choose the expected
   quality and cost level.
7. As a reviewer, I want the complete saved Custom connection loaded into its settings
   form, so that I can view and edit every field together.
8. As a reviewer, I want the API key hidden by default and revealable with a password
   visibility control, so that I can verify it when needed.
9. As a reviewer, I want one save operation to replace the complete connection, so that
   partially patched settings cannot describe an ambiguous service.
10. As a reviewer, I want Custom selected by default without contacting a service, so
    that Narrant does not consume Codex or Claude Code quota before I explicitly choose
    it.
11. As a reviewer, I want an unconfigured Custom provider to explain that setup is
    required and offer an explicit settings action, so that it never silently falls
    back to another provider.
12. As a reviewer, I want Custom to explain the whole File Change with the same Line
    Explanation contract as installed providers, so that the Review Reader behaves
    consistently.
13. As a reviewer, I want the agent to discover and read relevant repository context,
    so that explanations are not limited to the selected diff.
14. As a security-conscious reviewer, I want repository exploration confined to the
    active Review Workspace, so that model-directed tools cannot read another path.
15. As a reviewer, I want large tool results explicitly truncated, so that one search
    cannot exhaust the model context invisibly.
16. As a reviewer, I want cancellation to stop local tool work, reject late output, and
    prevent incomplete caching, so that cancelled analysis cannot reappear.
17. As a reviewer, I want malformed final output repaired in the same in-memory agent
    conversation, so that repeated prompts may reuse provider caches without weakening
    Narrant validation.
18. As a reviewer, I want connection changes to invalidate completed Custom results but
    not mutate an in-flight request, so that each analysis uses one frozen identity.
19. As a security-conscious reviewer, I want the API key persisted only by the operating
    system credential store, so that it never enters settings.json or SQLite.
20. As a maintainer, I want Rig hidden behind the existing Babysit provider seam, so
    that replacing the library does not change settings identity or Review behavior.

## Implementation Decisions

- The stable provider identity is `custom`; its localized UI label is Custom/自定义.
  Rig is an implementation detail rather than a provider identity.
- Application Settings retain exactly one Model Service Connection for Custom. The
  connection contains protocol, base URL, model identifier, effort, and an optional
  credential.
- The protocols are explicit `openai_chat_completions` and `anthropic_messages` values.
  Narrant does not inspect the URL or model name to infer a protocol.
- The URL is a base URL. Narrant passes it to the selected Rig client without a scheme,
  host, redirect, proxy, or endpoint allowlist. Rig owns endpoint path construction and
  its default HTTP behavior.
- The key is passed to the selected Rig client, which owns the standard protocol
  authentication header. An empty key represents no credential.
- A dedicated typed settings read returns the full connection, including the current
  key, only for the Custom editing flow. A dedicated typed save receives the complete
  protocol, URL, key, model, and effort value.
- The Custom form keeps the key only in component memory, uses a password textbox with
  a visibility control, and never writes it to browser persistence, URLs, global state,
  logs, or diagnostics.
- Non-secret settings are saved first, followed by credential replacement. If the
  credential operation fails, the backend restores the preceding non-secret settings
  and reports one failed save.
- Fresh Application Settings select Custom. Missing URL or model leaves Custom
  unavailable, never falls back, and returns an explicit user-triggered settings
  recovery action.
- The user enters an arbitrary non-empty model identifier. Custom does not fetch or
  validate a model catalog when settings are saved.
- Effort is `low`, `medium`, or `high`, defaults to `medium`, and is part of the frozen
  analysis and cache identity. OpenAI Chat Completions maps it to `reasoning_effort`.
  Anthropic Messages maps it to `output_config.effort` and also requests adaptive
  thinking.
- Saving settings performs local validation only. Protocol, authentication, model, tool,
  and schema compatibility errors surface on the first real analysis request.
- Rig registers only Narrant-owned Read, Glob, and Grep repository tools plus an
  internal schema-bearing final-output tool. No shell, write, edit, Web, MCP, or generic
  filesystem tool is registered.
- Read and Grep resolve actual filesystem paths with `canonicalize` and accept them only
  when `strip_prefix` proves they remain below the canonical Review Workspace. Glob
  starts at the workspace root and applies the same check to each match.
- Read returns at most 64 KiB per call with offset/limit continuation. Glob returns at
  most 500 paths and Grep at most 500 matching lines. Every truncated result says so.
- The final Line Explanations use Rig Tool output mode for both protocols. Narrant still
  parses, validates, and merges repair patches through its provider-independent contract.
- Initial generation and up to three repairs reuse one in-memory Rig conversation. Each
  generation or repair permits at most 16 model turns, while the whole File Change run
  retains the existing ten-minute deadline.
- Cancellation drops the active HTTP future, stops further local tool dispatch, rejects
  late output, and prevents incomplete caching. Remote termination and billing are best
  effort rather than a product guarantee.
- A connection snapshot freezes protocol, URL, model, effort, and credential generation
  for an in-flight run. A successful connection change invalidates completed Custom
  caches; the credential itself never enters a cache key.
- Diagnostics include only Custom identity, protocol, phase, HTTP status category,
  redacted Rig error category, and an irreversible endpoint-origin hash. They exclude
  URL, key, request/response bodies, prompts, source, Line Explanations, and tool output.

## Testing Decisions

- The highest behavioral seam is the existing Babysit application and provider registry.
  The reusable provider contract suite must run against Custom with deterministic fake
  HTTP responses rather than real paid requests.
- Custom connection tests exercise the typed settings read/save seam with an in-memory
  credential adapter, including complete round trips, empty credentials, default Custom,
  rollback after credential failure, and cache invalidation.
- Rig transport tests use an injectable recording HTTP backend or deterministic local
  server and assert the final OpenAI and Anthropic request shapes without recording
  secrets or repository content.
- Repository tool tests use real temporary directory trees and cover canonical paths,
  `..`, absolute inputs, symlinks inside and outside the workspace, missing paths,
  truncation, continuation, cancellation, and deterministic ordering.
- Provider tests cover complete output, final-tool schema errors, partial repair patches,
  three exhausted repairs, tool-loop limits, timeout, cancellation at model and tool
  boundaries, late responses, frozen connection identity, and completed-only caching.
- React behavior tests cover selecting Custom, the full form, password visibility,
  validation, save failure, unconfigured recovery, and absence of silent fallback.
- The Preview Harness is inspected in explicit wide and narrow viewports with empty,
  configured, invalid, saving, failed, and password-revealed states.
- CI performs no live model request. A documented opt-in smoke procedure may exercise
  reviewer-supplied OpenAI-compatible and Anthropic-compatible endpoints without
  printing secrets, prompts, source, responses, or full URLs.

## Out of Scope

- More than one saved Custom connection or named connection profiles.
- Automatic protocol detection, endpoint discovery, or model listing.
- Responses API, provider-specific protocols, arbitrary request headers, query-token
  authentication, or arbitrary model parameters.
- Narrant-hosted inference, accounts, quotas, billing, credential proxying, or a bundled
  local model.
- Guaranteeing remote cancellation, remote cache hits, or compatibility with every
  server that describes itself as OpenAI- or Anthropic-compatible.
- Shell, write, edit, Web, MCP, vector retrieval, or model-hosted repository tools.
- Persisting conversations or Line Explanations outside the existing process lifetime.

## Further Notes

- The governing architectural decision is
  [`ADR-0026`](../adr/0026-use-rig-for-the-custom-babysit-provider.md).
- This specification extends the provider-independent contracts in
  [`0012-babysit-mode.md`](./0012-babysit-mode.md); where provider-specific details
  differ, this document governs Custom.

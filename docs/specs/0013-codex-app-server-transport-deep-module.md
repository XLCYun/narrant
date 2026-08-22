# Codex App Server Transport Deep Module

## Problem

Babysit Mode currently owns Codex App Server process I/O, JSONL framing, JSON-RPC
correlation, server-message routing, Codex endpoint orchestration, and recovery
classification in one `transport.rs` module. The behavior is covered by protocol
fixtures, but the mixed responsibilities make cancellation, compatibility, and cleanup
changes difficult to reason about independently.

## Scope

Refactor the installed Codex provider into an internal deep module with three layers:

1. `JsonlConnection` owns the child process streams, newline-delimited JSON framing,
   bounded reads and writes, deadlines, cancellation observation, and process shutdown.
2. `RpcSession` owns sequential request identifiers, response correlation, notification
   buffering, and classification of server requests.
3. The Codex protocol facade owns initialization, account checks, thread and turn
   operations, capability probes, cleanup policy, endpoint DTOs, and translation from
   transport/RPC failures into Babysit recovery errors.

The refactor must preserve the current provider interface and observable behavior.

## Non-goals

- Adding or removing Codex App Server endpoints.
- Changing discovery order, model selection, reasoning effort, permissions, approval
  policy, timeouts, cancellation semantics, thread ownership, or cleanup behavior.
- Supporting concurrent in-flight RPC requests on one App Server connection.
- Adding `jsonrpsee`, Codex monorepo crates, or another JSON-RPC dependency.
- Changing the Tauri or frontend Babysit contracts.
- Browser-based UI acceptance; this change has no visual behavior.

## Decisions

### Module layout

```text
src-tauri/src/babysit/codex/transport/
├── jsonl_connection.rs
├── rpc_session.rs
└── mod.rs
```

The module remains private to the installed Codex provider. `codex.rs` must not read or
write JSONL or correlate JSON-RPC identifiers directly.

### Sequential RPC

The existing App Server process mutex continues to serialize access. `RpcSession` may
therefore wait for one response while buffering interleaved notifications; it does not
need a background reader task or a map of concurrent pending requests.

### Error ownership

`JsonlConnection` and `RpcSession` report mechanism-level failures such as I/O, timeout,
cancellation, process exit, invalid JSON, malformed envelopes, and remote RPC errors.
Codex-specific code alone maps those failures to signed-out, incompatible,
initialization, provider, or cancelled Babysit errors.

### Bidirectional messages

`RpcSession` distinguishes responses, notifications, and server requests. It exposes
server requests to the Codex protocol facade, which preserves the current policy of
replying with JSON-RPC error `-32601`. Supporting a server request in the future changes
the Codex facade rather than the JSONL connection.

### Typed compatibility

Local typed envelopes represent request, response, notification, and server-request
shapes without requiring the omitted `jsonrpc` member. Endpoint DTOs cover only the
Codex methods currently used by Babysit Mode. DTOs tolerate unknown fields, unknown
notifications remain ignorable, and missing required fields remain protocol failures.
An explicit `serde_json::Value` escape hatch is allowed for evolving or intentionally
opaque payloads.

## Delivery slices

### JsonlConnection

- Move child stdin/stdout ownership, capped newline framing, bounded write/flush,
  cancellation polling, deadline enforcement, and child shutdown into
  `jsonl_connection.rs`.
- Preserve redaction: raw stderr, malformed lines, paths, and process output must not
  enter user diagnostics.
- Preserve partial-construction child reaping and blocked-I/O cancellation behavior.

### RpcSession

- Add typed wire envelopes compatible with Codex's omitted `jsonrpc` field.
- Move request ID allocation, response matching, remote-error extraction, notification
  buffering, and server-request classification into `rpc_session.rs`.
- Keep exactly one in-flight request per session and preserve unknown-notification
  behavior.

### Codex protocol facade

- Keep initialize/initialized, `account/read`, capability probes, thread lifecycle,
  turns, interruption, deletion, and orphan cleanup in `transport/mod.rs`.
- Replace direct envelope inspection with endpoint DTOs where fields are stable and
  required by current behavior.
- Preserve all public/provider seams and typed Babysit error mappings.

## Cross-ticket acceptance criteria

- Existing Babysit provider and frontend contracts are unchanged.
- Existing discovery, authentication, permission, capability, cancellation, repair,
  timeout, orphan cleanup, and shutdown tests retain their behavioral assertions.
- New focused tests cover each extracted layer through its private test module under
  `src-tauri/src/babysit/tests/codex/transport/`.
- Production structs, enums, and methods have readable Chinese comments.
- No production module keeps private unit tests in the same file.
- No new runtime dependency is added.
- `cargo fmt --check`, `git diff --check`, focused transport tests, and one final
  `cargo test --manifest-path src-tauri/Cargo.toml` pass, apart from a separately
  identified pre-existing flaky test that passes independently.


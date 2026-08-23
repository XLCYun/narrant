---
status: accepted
---

# Use stateless Claude Code repair rounds

The installed Claude Code provider starts every generation and repair round as a fresh
process with session persistence disabled. Narrant keeps the request, previous output,
validation failure, and repair count only in memory and sends each repair as a
self-contained request. This deliberately trades repeated input tokens and some repair
latency for keeping Babysit operations out of Claude Code's local session history: the
CLI has no supported single-session deletion operation, its raw persistent-stream
protocol is not a stable public integration contract, and the official Agent SDK has no
Rust implementation and cannot reuse Claude subscription authentication in a third-party
product without prior Anthropic approval.

## Consequences

- Provider-independent validation, repair limits, progress, cancellation, deadlines,
  and completed-result caching remain unchanged; only Claude's internal repair-session
  continuity differs.
- Completed results retain the same cache identity and hit behavior. Invalid intermediate
  outputs are never cached, while a repair round resends enough context to reconstruct
  the task.
- Narrant never deletes guessed files from Claude Code's private storage. A future
  stateful non-persistent protocol may replace this design only when it has a supported
  lifecycle and compatible authentication policy.

---
status: accepted
---

# Separate review models from command responses

Review acquisition models must not double as serialized command responses. Local Git
and GitHub keep source-specific snapshots and File Changes behind their adapters, while
the command/presentation seam maps them into tagged frontend responses; this adds
explicit mapper code but prevents frontend nullability, serialization, and transport
identity from shaping the underlying review model. A Review Session owns the
`sessionId`; its immutable Review Snapshot carries no runtime identity.

# Issue 139B: Custom provider registry readiness

Implement the provider-registry half of Issue #139 through the public Babysit
application seam.

- Register `custom` without network I/O or live endpoint probing.
- Project readiness from the selected Custom settings snapshot: protocol, base URL,
  and non-empty model form a configured provider; missing URL/model produces the
  existing unconfigured/setup failure and never falls back to another provider.
- Keep in-flight selection frozen under the existing settings contract.
- Do not implement Rig requests, model listing, or protocol probes.
- TDD configured/unconfigured selection and no-fallback behavior through public seams;
  avoid private map/layout assertions. Add readable Chinese comments and separate Rust
  tests. Run focused Cargo commands only via `scripts/cargo-agent.sh`; do not run the
  full Rust suite.


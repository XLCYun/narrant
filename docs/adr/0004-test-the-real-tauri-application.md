---
status: accepted
---

# Test the real Tauri application with WebdriverIO

Use WebdriverIO with `@wdio/tauri-service` and its embedded driver to run end-to-end tests against the actual Tauri binary on macOS and Windows. Browser-only tests cannot validate the system WebView, Tauri IPC, or Rust backend; test-only driver plugins must be excluded from production builds.

## Consequences

- Exercise local Git scenarios end to end against repository fixtures and the real libgit2 backend.
- Point test builds at a local mock GitHub server for deterministic GitHub and authentication scenarios without accessing real accounts.
- Keep fast Rust and React tests below the E2E layer for focused failures.

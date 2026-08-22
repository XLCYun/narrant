---
status: accepted
---

# Reuse the reviewer's Codex environment

Narrant starts its own `codex app-server` process but lets that process inherit the
reviewer's Codex home, authentication, and configuration. Discovery tries the bare
`codex` command from the application environment first, then a previously selected
executable and platform installation locations. Narrant does not read or copy Codex
credentials; it checks account state through the App Server protocol instead.

## Consequences

- User MCP servers, plugins, hooks, skills, personal instructions, notifications, and
  Codex history may participate in App Server startup and analysis.
- Babysit threads still use a Narrant-owned read-only permission profile, deny network
  access and approval escalation, and are deleted after use.
- A selected executable remains a fallback and never replaces a working bare `codex`.
- The temporary Codex home approach is deliberately set aside. Its original process
  isolation is recoverable from `43bbcf6f3414a5cb9b07c99b612fd454fb947c2c`,
  Keychain credential bridging from `84035d45be15684ea4969319d6ba917c60775ddc`,
  and related protocol hardening from `75ade3a`.

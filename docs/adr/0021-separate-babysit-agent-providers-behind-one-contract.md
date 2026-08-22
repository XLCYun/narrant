---
status: accepted
---

# Separate Babysit agent providers behind one contract

Babysit Mode supports Installed Coding Agent Providers and Built-in Agent Providers
behind one provider-independent Line Explanation contract. The first vertical slice
uses the reviewer's installed Codex App Server; a Claude Code adapter may implement the
same contract later. Rig is the leading candidate for a future Built-in Agent Provider,
but remains a separately tracked implementation decision. Narrant owns line identities,
coverage validation, cancellation, and the three-round repair policy so provider
protocols cannot change Review Reader behavior or weaken Workspace Trust.

## Consequences

- Installed and Built-in providers may use different authentication, process, event,
  and model protocols without exposing those differences to the Review Reader.
- Provider selection and analysis caching include the provider identity and analysis
  contract version.
- Supporting another provider requires a contract adapter and the same behavioral test
  suite; it does not introduce provider-shaped Line Explanation models.

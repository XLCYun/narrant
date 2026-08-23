---
status: accepted
---

# Prefer an explicit Codex executable

When a reviewer configures a **Codex Executable Preference**, Babysit Mode uses only
that executable and reports an actionable error if it is unavailable or invalid. It
does not fall back to another Codex installation because silently ignoring an explicit
choice would make the displayed setting differ from the runtime actually used. Clearing
the preference restores automatic discovery through the application environment and
platform installation locations. Codex executable selection is configured only through
Application Settings, not as an error-recovery action in the Review Reader.

This supersedes ADR-0022 only where that decision made a selected executable a fallback
behind a working bare `codex`; reusing the reviewer's Codex home, authentication, and
configuration remains unchanged.

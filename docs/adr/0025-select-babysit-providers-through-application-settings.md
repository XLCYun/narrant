---
status: accepted
---

# Select Babysit providers through Application Settings

Babysit Mode stores one device-level provider choice in Application Settings and keeps
independent executable, model, and reasoning-effort preferences for Codex and Claude
Code. A provider registry resolves the selected provider into one capability bundle
for readiness, model catalog, analysis, recovery, and shutdown; Review and transport
requests never select a provider themselves. This trades a larger refactor now for
removing Codex-shaped branches before a second production provider makes them part of
the public contract, while preserving an in-flight analysis as a frozen provider and
model snapshot when settings change.

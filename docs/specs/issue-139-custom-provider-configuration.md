# Issue 139: Custom Provider Configuration

## Problem Statement

Custom cannot become a predictable Babysit provider until the reviewer can configure
one complete model-service connection and Narrant can keep its credential consistent
with the non-secret Application Settings snapshot.

## Solution

Add Custom to the provider registry and Application Settings, select it for fresh
settings, and provide a dedicated editor that loads and saves the complete protocol,
base URL, API key, model, and effort value. Custom remains unavailable until configured
and offers an explicit settings recovery action without falling back.

## User Stories

1. As a reviewer, I want Custom selected without making a request, so that no installed
   provider quota is consumed without my choice.
2. As a reviewer, I want to edit the complete connection in one form, so that its fields
   cannot come from different saves.
3. As a reviewer, I want to reveal the loaded key when needed, so that I can verify it.
4. As a reviewer, I want an optional key, so that local unauthenticated services work.
5. As a reviewer, I want a visible settings action when Custom is incomplete, so that I
   can recover without silent fallback.
6. As a reviewer, I want a failed credential update to preserve my preceding connection,
   so that settings and keychain never disagree.

## Implementation Decisions

- The provider identity is `custom`; UI labels are Custom/自定义.
- The complete connection contains explicit protocol, base URL, key, non-empty model,
  and low/medium/high effort with medium as the default.
- Dedicated typed read/save commands carry the full connection for the Custom editor.
  The key may cross this in-memory IPC but never enters settings.json or SQLite.
- Saving writes non-secret settings first and then replaces or deletes the credential.
  Credential failure restores the preceding settings snapshot.
- The key field uses the repository's shadcn-based input conventions, defaults to
  password display, and has an accessible visibility button.
- Missing URL or model maps to an unconfigured setup failure with an explicit, user-
  triggered Application Settings recovery action.
- Provider selection never silently falls back. Existing in-flight behavior remains
  frozen by the existing settings contract.

## Testing Decisions

- Exercise the Application Settings application and typed command seam with an in-memory
  credential adapter, including complete round trips, empty key, replacement, deletion,
  rollback, and fresh defaults.
- Exercise the provider registry readiness projection for configured and unconfigured
  Custom without network I/O.
- React behavior tests cover selection, editing, visibility, validation, saving, failure,
  recovery, localization, and preservation of form input after a failed save.
- Use `pnpm dev:preview` for wide and narrow empty/configured/revealed/failed states and
  inspect screenshots, accessible DOM, and relevant element dimensions.

## Out of Scope

- Rig model requests, repository tools, protocol compatibility probes, multiple saved
  connections, model listing, or live endpoint validation.

## Further Notes

- Governed by `docs/specs/0015-custom-babysit-provider.md` and ADR-0026.

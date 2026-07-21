---
status: accepted
---

# Use a GitHub App with Device Flow

Authenticate private-repository access through a GitHub App using Device Flow, with repository permissions limited to read-only metadata, contents, and pull requests. This adds an installation and device-authorization step, but unlike a traditional OAuth App with the broad `repo` scope, it preserves the product's read-only boundary, lets users or organizations select accessible repositories, and avoids embedding a client secret in the desktop application.

## Consequences

- The application must guide users through both GitHub App installation and device authorization when required.
- Organization policy may require an owner to approve the installation before private repositories become accessible.
- Enable expiring user access tokens; the Rust side refreshes them with rotating refresh tokens and requests authorization again only when refresh fails.
- Access tokens, refresh tokens, and expiry metadata must remain in the Rust side and be stored in the operating system credential store.
- Treat the operating-system user as the local security boundary; signing out removes stored credentials and GitHub recent entries so switching GitHub accounts within one OS account does not expose private PR locations.

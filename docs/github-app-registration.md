# Register the Narrant GitHub App

GitHub App registration is a maintainer operation. Narrant release users do not set
environment variables and must never receive a client secret or private key.

## GitHub settings

1. Register a GitHub App owned by the release account or organization.
2. Keep expiring user access tokens enabled.
3. Enable **Device Flow**.
4. Set repository permissions to:
   - **Metadata**: Read-only
   - **Contents**: Read-only
   - **Pull requests**: Read-only
   - every other permission: No access
5. Disable webhooks because Narrant does not receive events.
6. Make the app public when accounts outside the owner need to install it.

Use the installation page to validate personal-account repository selection and an
organization whose policy requires owner approval. Automated tests use local mock
servers and must not access a real GitHub account.

## Build configuration

Set these public values when compiling the Tauri application:

```sh
NARRANT_GITHUB_CLIENT_ID=Iv1.example \
NARRANT_GITHUB_APP_SLUG=narrant-example \
pnpm tauri build
```

- `NARRANT_GITHUB_CLIENT_ID` is the Client ID shown on the GitHub App settings page.
- `NARRANT_GITHUB_APP_SLUG` is the path segment in
  `https://github.com/apps/<slug>/installations/new`.

Do not configure `CLIENT_SECRET` or a GitHub App private key. A build without both
public values still allows opening a Review Session for a public Pull Request
anonymously and reports that GitHub login is not configured.

# Global GitHub Identity Entry

## Problem

Narrant currently exposes GitHub connection controls inside repository-specific UI,
even though GitHub authorization belongs to the application as a whole. The UI also
reports only whether a credential exists, so a reviewer cannot confirm which GitHub
user is currently authorized.

Narrant needs one stable, global identity entry that identifies the authorized GitHub
user, starts authorization when signed out, and provides the sign-out action when
signed in.

## Scope

- Continue to support at most one authorized GitHub user at a time.
- Fetch the authenticated user's public profile after authorization.
- Present the GitHub identity in a global entry fixed to the bottom of the left sidebar.
- Cache enough profile and avatar data to preserve the identity presentation while
  GitHub is temporarily unavailable.
- Consolidate persistent GitHub connection and sign-out controls into the global entry.

## Domain

The current authorized user is the **GitHub Identity** defined in
[`CONTEXT.md`](../../CONTEXT.md). It is an external GitHub authorization identity, not a
Narrant-owned account or profile.

Profile presentation and authorization state are separate concerns. Failure to refresh
a name or avatar does not invalidate otherwise usable credentials.

## Experience

### Signed out

The global entry remains visible in its fixed sidebar position. It displays a GitHub
icon and the label “Connect GitHub”. Activating it starts the existing GitHub
authorization flow.

### Signed in with profile data

The global entry displays the GitHub avatar and one compact name:

1. Use the GitHub profile `name` when it is non-empty.
2. Otherwise use the GitHub `login`.

Activating the entry opens a menu. Its non-action header displays the avatar, compact
name, and `@login` so that users can distinguish accounts with the same display name.
The only current action is “Sign out of GitHub”.

Application settings do not appear in this menu yet. A future settings feature may add
an entry here without changing the identity model.

### Signed in without profile data

If the initial profile request fails and no cached profile exists, the entry displays
the default GitHub icon and “GitHub connected”. Its menu still offers “Sign out of
GitHub”. Profile failure must not be presented as an authentication failure.

### Avatar fallback

If the avatar is unavailable, invalid, or cannot be decoded, display an initials
placeholder derived from `name`, falling back to `login`. A broken image indicator must
not be shown.

## Profile Lifecycle

- Immediately after successful authorization, request the authenticated user with the
  current GitHub user access token.
- On application startup, render cached profile data first and refresh it once in the
  background.
- Do not refresh profile data merely because the user navigates between views.
- Persist the GitHub numeric user ID, `login`, optional `name`, and locally cached avatar
  reference. The numeric ID identifies the profile independently of mutable names.
- Download, validate, and cache the avatar on the Rust side. The WebView displays only
  the local resource and does not load the GitHub avatar URL directly.
- Bound avatar downloads by an explicit size limit, accept only supported image content,
  and replace cached avatar data atomically after successful validation.
- Signing out removes credentials, pending authorization, cached profile metadata, and
  cached avatar data.
- Connecting another account requires signing out first. There is no separate account
  switcher.

## Existing Entry Points

- Remove duplicate persistent “Connect GitHub”, “GitHub connected”, and “Sign out of
  GitHub” controls from repository navigation.
- Preserve contextual prompts such as the private Pull Request empty state and its
  connection action, because those prompts explain why content is unavailable.
- The global entry appears throughout the main application rather than belonging to a
  particular Repository Entry or Review.

## Public Application Contract

The frontend-visible authorization state must be able to represent:

- whether the GitHub App is configured;
- whether a GitHub credential is present;
- the optional cached/refreshed public identity containing numeric ID, `login`, optional
  `name`, and a safe local avatar reference.

Access tokens, refresh tokens, remote avatar bytes, credential expiry values, and
credential-store details remain outside the WebView contract.

## Acceptance Criteria

1. A signed-out user sees the global “Connect GitHub” entry in every main application
   view and can start the existing authorization flow from it.
2. After authorization, the entry identifies the authorized GitHub user without
   requiring an application restart.
3. A user with a display name sees that name in the compact entry; a user without one
   sees their GitHub login.
4. The opened menu shows `@login` and only the “Sign out of GitHub” action.
5. Two GitHub users with the same display name remain distinguishable in the menu.
6. Cached identity presentation is available before a startup refresh completes and
   remains usable when GitHub is temporarily unreachable.
7. Missing or invalid avatars render an initials placeholder.
8. Profile or avatar refresh failure does not change a valid signed-in state.
9. Signing out immediately removes both credential and cached identity data and returns
   the entry to “Connect GitHub”.
10. Persistent repository-level GitHub controls are removed, while contextual
    connection prompts remain.
11. Automated tests cover signed-out, signed-in, missing-name, missing-profile,
    avatar-fallback, offline-cache, refresh, and sign-out behavior.

## Non-goals

- Multiple simultaneously authorized GitHub users.
- A one-step account-switching action.
- A Narrant-owned user account or profile.
- Editing GitHub profile data.
- Adding application settings before a settings feature exists.
- Changing GitHub App repository permissions or the existing Device Flow.


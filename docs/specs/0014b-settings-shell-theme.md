# Settings Shell and Theme Slice

## Problem Statement

Narrant has no discoverable Application Settings destination, and appearance follows the operating system without an explicit persistent preference. The shell, Monaco, and brand imagery can therefore drift between independent theme signals.

## Solution

Add a round accessible Settings button beside GitHub Identity, an independent settings destination with extensible secondary navigation, and immediate system/light/dark theme selection backed by the public Application Settings contract.

## User Stories

1. As a user, I want a Settings button beside GitHub Identity, so that settings are reachable from every primary destination.
2. As a keyboard or assistive-technology user, I want a named, focusable Settings control, so that I can operate it reliably.
3. As a user, I want secondary navigation for Appearance, GitHub, and Babysit sections, so that future settings can grow without redesigning navigation.
4. As a user, I want returning from Settings to restore my prior Home, Repository Overview, or Review Reader destination.
5. As a user, I want system, light, and dark theme choices to apply immediately and persist.
6. As a user, I want persistence failure to restore the previous theme and show an actionable error.
7. As a small-window user, I want secondary navigation and settings content to remain usable.

## Implementation Decisions

- Settings is an independent application-shell destination with remembered return destination.
- The sidebar footer places a round gear button with the accessible name “设置” beside GitHub Identity.
- Secondary navigation is present from the first release and contains Appearance, GitHub, and Babysit entries; Issue #99 fully implements Appearance while other sections may be stable placeholders for their dependent issues.
- Theme is one typed preference: system, light, or dark. Its effective value drives the root theme, Monaco, and brand imagery.
- Theme changes render immediately, persist through the Application Settings contract, and roll back visibly on failure.
- New or modified methods, structures, and classes receive readable Chinese comments.

## Testing Decisions

- Test at the application-shell UI seam, covering entry, accessible naming, return restoration, secondary navigation, immediate theme behavior, persistence rollback, and system-theme changes.
- Do not test private component helpers.
- Use red-green vertical slices and focused frontend tests during implementation.
- Use `pnpm dev:preview` and the in-app browser for wide and narrow viewport rendering, navigation, interaction, and error states; restore the default viewport afterwards.

## Out of Scope

- GitHub sign-out implementation, Codex executable selection, and dynamic model configuration.

## Further Notes

- This slice consumes the public contract delivered by the parallel Application Settings Contract slice and is governed by `docs/specs/0014-application-settings.md`.

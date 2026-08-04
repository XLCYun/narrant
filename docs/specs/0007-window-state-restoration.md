# Window State Restoration

Narrant currently starts every desktop session at the configured default size, which discards a reviewer’s working layout. The application must restore a safe, user-selected desktop window state across launches without mixing it into Review or Repository Entry data.

## Scope

- Use Tauri’s official `window-state` plugin to persist the main window’s normal width, height, position, maximized state, and visibility during an orderly application exit.
- When restarting from a maximized state, let the plugin restore the saved normal bounds before maximizing so a later restore returns to the user’s prior bounds.
- Do not persist or restore fullscreen state.
- Start the configured window hidden so the plugin can restore it before showing it, preventing visible movement at launch.
- Rely on the plugin’s built-in monitor-intersection check: when the saved position does not intersect an available display, do not restore that position and let the operating system choose placement. A malformed state file is treated as no saved state.
- Keep this application-level state separate from the Review catalog and frontend layout preferences.

## Non-goals

- Do not add custom state-file management, debounce writes, position correction, display-work-area clamping, or a user-facing reset action.
- Do not guarantee recovery from an abrupt process termination before the plugin’s orderly-exit save.

## Acceptance Criteria

1. A normal main window reopened after an orderly exit retains its last usable size and position.
2. A maximized main window reopens maximized; restoring it returns to its prior normal bounds.
3. Fullscreen is not restored.
4. A saved position outside all available displays is not applied; a malformed state file falls back to the configured default behavior.
5. The configured minimum size is 900×600 and the initial default remains 1100×760 when no valid saved state exists.
6. Static integration tests verify the selected plugin state flags and hidden initial window; real Tauri E2E covers normal and maximized relaunch behavior.

# Issue 139C: Custom provider settings UI

Implement the React/UI half of Issue #139 through
`ApplicationSettingsApplication` and SettingsPage behavior.

- Extend typed frontend settings with provider `custom` and a dedicated complete
  connection read/save operation (protocol, base URL, optional API key, non-empty
  model, low/medium/high effort defaulting to medium).
- Show Custom/自定义 in provider selection and a dedicated editor. Use repository
  shadcn components first. The key is password-hidden by default and has an accessible
  reveal/hide button.
- Validate required URL/model, save atomically as one form, preserve entered fields on
  failed save, and show a clear error. An incomplete selected Custom provider exposes
  an explicit user-triggered Application Settings recovery action with no fallback.
- Add behavior tests for selection, editing, reveal, validation, saving, failure,
  recovery, and localization. Work TDD in vertical slices.
- Extend the shared `pnpm dev:preview` harness with empty/configured/revealed/failed
  representative states. Do not perform browser visual acceptance; the drone will do
  final wide/narrow screenshots, accessible DOM, and dimension checks after merge.


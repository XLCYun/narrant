# Issue 139A: Custom settings and credential contract

Implement the backend settings half of Issue #139 through the public
`ApplicationSettingsApplication` and typed Tauri-command seams.

- Add stable provider identity `custom`; fresh settings select Custom and retain Codex,
  Claude Code, and Custom configurations independently.
- A Custom connection contains protocol, base URL, non-empty model, and
  low/medium/high effort (default medium). The API key is optional and must never be
  serialized to settings.json or SQLite.
- Add typed read/save commands carrying the complete in-memory connection.
- Use an injectable credential adapter and production OS credential implementation.
- Save non-secret settings first, then replace/delete the credential. If credential
  mutation fails, restore the exact preceding settings snapshot and report failure.
- TDD through public application/command seams: defaults, complete round trip, empty
  key, replace, delete, failed credential rollback, validation, and secret absence.
- Add readable Chinese comments for new/changed Rust structures and methods. Keep Rust
  tests in module `tests/` files. Run focused Cargo commands only via
  `scripts/cargo-agent.sh`; do not run the full Rust suite.


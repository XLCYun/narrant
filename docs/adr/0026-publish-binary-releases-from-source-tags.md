# Publish binary releases from source tags

Narrant treats a stable `vMAJOR.MINOR.PATCH` tag in the private source repository as
the authoritative release request and version, regardless of which branch contains the
tagged commit. A release proceeds only when that version matches `package.json`, the
Tauri configuration, `Cargo.toml`, and the Narrant package entry in `Cargo.lock`, and
only after the reusable quality gate and both the Apple Silicon macOS and x64 Windows
builds succeed. The public `cyunlab/narrant` repository is an artifact channel rather
than a source mirror: automation uses repository secrets and a narrowly scoped
fine-grained token to write a complete draft Release there, without exposing private
source identity. A maintainer reviews the generated commit-title notes and assets and
publishes the draft manually. Windows artifacts may be unsigned for now; macOS
artifacts remain signed and notarized.

## Consequences

- A public Release tag identifies the artifact release, not the private source commit;
  neither the private commit SHA nor a private repository link is included.
- Failed runs may retain a draft Release and replace its assets on retry, but automation
  must never modify an already published Release.
- Release preparation needs one command that keeps all four version declarations in
  sync before an annotated source tag is created. Since a tag can request a release from
  any commit, control of stable-version tags is control of the release channel.

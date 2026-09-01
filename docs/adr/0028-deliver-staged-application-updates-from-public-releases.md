# Deliver staged Application Updates from public Releases

Narrant uses published Releases in the public `cyunlab/narrant` artifact repository as
its Stable Application Update channel while the private source repository remains the
authority that builds and signs each release. The client provides the full staged update
lifecycle proven in DSH Desktop—automatic periodic checks and downloads, durable verified
staging, explicit restart or installation during normal application exit, platform-aware
installation with recovery, and native upgrade smoke coverage—rather than exposing the
stock updater plugin directly. This preserves a public anonymous update feed without
turning the public repository into a source mirror or introducing a separate OSS promotion
service.

## Consequences

- A complete Draft Release contains the platform updater packages, their signatures, and
  `latest.json`. A manually approved, protected promotion job revalidates the Draft and
  its native-upgrade evidence before publishing it as the atomic promotion to Stable.
- The updater trust root is specific to Narrant. Its private key remains in the private
  source repository's protected release environment, while the public key is embedded in
  the application and may be published.
- The Stable manifest initially contains only the shipped `darwin-aarch64` and
  `windows-x86_64` targets.
- The first updater-capable release is a one-time manual installation for existing users;
  every subsequent Stable release can be delivered as an Application Update.
- GitHub Release immutability is not a release prerequisite. Automation still refuses to
  modify a published Release, while repository administrators retain GitHub's ability to
  change its tag or assets.
- The application checks on startup and every six hours, downloads updates automatically
  by default, keeps verified staging across launches, and installs a staged update during
  normal exit without forcing an in-use application to restart. Reviewers can disable
  automatic download or explicitly choose to restart and update immediately.
- Exit preparation stops new operations, cancels all registered work, and gives the shared
  application cleanup path one total fifteen-second deadline. If cleanup does not finish,
  Narrant keeps the staged update, skips installation, and allows the process to exit.
- Normal exit and explicit update restart reuse one idempotent application-stop boundary;
  Application Update does not duplicate Review or Babysit provider cleanup. A successful
  stop may be followed by installation, while a timed-out stop must never install.
- Application Update controls live in Application Settings. While an update needs the
  reviewer's attention, a small circular UI Icon appears immediately to the right of the
  GitHub Identity entry in the sidebar. It remains visible while an update is available,
  downloading, failed, or staged and opens the in-application update surface without
  immediately restarting Narrant.

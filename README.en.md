<div align="center">
  <img src="./assets/brand/narrant-app-icon.svg" width="160" alt="Narrant Logo">
  <h1>Narrant</h1>
  <p><strong>Good engineering welcomes sound counsel.</strong></p>
  <p>Code review should not become the AI bottleneck. Let AI accelerate human review, and work with AI to guard the final gate.</p>
  <p><a href="./README.md">中文</a> · <a href="#babysit-mode">Babysit Mode</a> · <a href="#download">Download</a> · <a href="#getting-started">Getting Started</a> · <a href="#features">Features</a> · <a href="#feedback">Feedback</a></p>
</div>

## About Narrant

AI coding is accelerating software development, but it also creates more changes for people to review. For important code, human review remains the last line of defense before a change reaches the main branch. Reviewers must understand whether the code meets the real requirements and make informed decisions about architecture and risk.

Narrant is a desktop tool that uses AI to accelerate human review. It brings local branch comparisons and GitHub Pull Requests into one focused experience, with clear diffs and AI-assisted explanations. Engineers retain the final judgment.

![Narrant review overview](./assets/screenshots/narrant-overview.jpg)

## 👶 Babysit Mode

Babysit Mode uses your locally installed and authenticated Codex to generate line-by-line explanations for the current code change. You can focus on changed lines or expand the explanation to every non-empty line, with before-and-after code shown side by side. It helps you work through unfamiliar languages, complex logic, and large changes without repeatedly searching for context.

The explanations support your reading and do not approve a change or make the final review decision. Narrant skips binaries, dependencies, build artifacts, lockfiles, and other content that is unsuitable for line-by-line analysis. Install the Codex CLI and sign in before using this mode. When enabled, the current file's changes are provided to Codex for analysis.

![Narrant Babysit Mode showing line-by-line explanations in an inline diff](./assets/screenshots/narrant-babysit.jpg)

## ⬇️ Download

Visit [GitHub Releases](https://github.com/XLCYun/narrant/releases) and download the package for your operating system and processor architecture.

## Getting Started

1. Open Narrant and choose a local Git repository.
2. Read the workspace trust notice and confirm it to enter the repository.
3. Select Head and Base under Local Branches, or choose a review from Pull Requests.
4. Move between the review overview and file changes as you read.
5. Turn on Babysit Mode when you want help understanding a supported code file.
6. Refresh the review snapshot when Narrant detects new commits.

To review a private Pull Request, connect GitHub in the app and grant read access to the repository.

## ✨ Features

- **Review local branches.** Select Base and Head branches and inspect the complete change in its intended merge direction
- **Discover GitHub Pull Requests.** Find public Pull Requests through the local repository's GitHub remotes, or connect GitHub to access authorized private repositories
- **Read code without distractions.** Browse added, modified, deleted, and renamed files, then switch between inline and side-by-side diffs
- **Keep review history.** Save reviews and snapshots by repository, then explicitly refresh and continue when a branch or Pull Request changes
- **Chinese and English UI.** Follow the system language or switch languages in the app
- **Light and dark themes.** Follow the system appearance

## 🔒 Local-first Design and Security

Narrant reads the local Git repository you select and creates an app-managed temporary workspace for each review. The workspace is cleaned up when the app exits. Before opening a repository, Narrant asks you to confirm workspace trust because Git hooks, filters, and Git LFS configuration may execute programs or read external content.

Connecting GitHub is optional. Public Pull Requests can be discovered anonymously. For private repositories, Narrant uses GitHub's device authorization flow to request the read access you grant, then stores the credential in the operating system's credential store.

## Current Status

Narrant is under active development. Its interface, data format, and behavior may change in future releases. Release notes will describe significant changes and known limitations.

## Feedback

Please use [GitHub Issues](https://github.com/XLCYun/narrant/issues) to report a problem or suggest an improvement. Include your operating system, Narrant version, reproduction steps, and sanitized error details when possible. Do not upload access tokens, private code, or other sensitive data.

## Development and license

See the [development guide](docs/development.md). Narrant is licensed under [Apache-2.0](LICENSE); third-party components and assets retain their own license notices.

The GitHub Releases channel is being migrated and may remain empty until its first release. Existing users will need to install a fixed version from this repository manually once to switch to the new update channel.

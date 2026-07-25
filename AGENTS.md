## Agent skills

### Backlog

Implementation tickets live in this repository's GitHub Issues. Product requirements and
specifications live in `docs/specs/`. See `docs/agents/backlog.md`.

### Triage labels

Uses the default five-label vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout. See `docs/agents/domain.md`.

## 前端 UI 组件
1. 如果 shadcn 提供的默认组件满足要求，使用 shadcn 的（如果未安装下载组件，则下载后使用）
2. 否则，如果能在 shadcn 组件基础上定制，则定制。
3. 否则，才新增组件

## 风格指南
1. 对方法、结构体、类等，都需要添加易读的中文注释。

## 提交前代码格式化
1. Rust 代码使用 Rust 官方格式化工具 `rustfmt`，不要手工维护格式差异。
2. 修改 Rust 代码后、准备执行 `git commit` 前，在仓库根目录运行：
   `cargo fmt --manifest-path src-tauri/Cargo.toml --all`
3. 提交前运行格式检查，确认没有未格式化的代码：
   `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
4. `cargo fmt` 只负责代码格式化；提交前仍应根据改动范围运行必要的测试和 Clippy 检查。
5. 本仓库提供了版本化的 `pre-commit` hook。首次克隆或切换到本仓库后，在仓库根目录运行：
   `git config core.hooksPath .githooks`
   之后每次提交都会自动执行 Rust 格式检查；检查失败时，先运行第 2 条命令再重新提交。

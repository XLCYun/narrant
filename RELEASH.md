
# 1. 全量测试
## 1.1 Rust 门禁
```bash
cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --manifest-path src-tauri/Cargo.toml --doc
```
其中 Rust 测试应通过 `scripts/cargo-agent.sh` 获取仓库级全局锁；同一次发布只运行一次 Rust 全量测试。

## 1.2 TypeScript 门禁
```bash
pnpm install --frozen-lockfile
pnpm lint
pnpm build
pnpm test
```

## 1.3 真实 Tauri E2E

分别在 macOS 和 Windows 环境准备本地 Git fixture，然后运行：
```bash
node e2e/fixtures/setup-local-repo.mjs "<临时目录>/narrant-fixture"
pnpm test:e2e
```

# 2. 打新版本 release

当前仓库为 Private 仓库，实际的公共发布仓库在 https://github.com/cyunlab/narrant 。

当要发布一个新版本时，先检查当前仓库打的最的一个 tag 是 va.b.c：
1. 发大版本时，上升为 v{a+1}.b.c
2. 发中版本时，上升为 va.{b+1}.c
3. 发小版本时，上升为 va.b.{c+1}
确定后使用 `git tag` 打上新标签。

更新 package.json

# 3. 创建 release

查看从 va.b.c 到当前新版本的 git log 日志，总结版本变动内容。使用 `gh` 命令在 cyunlab/narrant 仓库创建一个新的 release，描述使用版本变动内容。

## 4. 发布 artifact

若当前在 macos 系统，使用命令 `pnpm run build:macos:signed` 构建新的带签名的 macos 安装包。

若当前在 windows 系统，使用命令 `pnpm run build:windows` 构建 windows 安装包。

将构建的安装包上传到 cyunlab/narrant 新创建的 release 中。

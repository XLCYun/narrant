# macOS 自托管 CI

`language-support-packs.yml` 通过仓库变量 `NARRANT_MACOS_RUNNER_LABELS` 选择
Apple Silicon runner。未设置变量时仍用 `macos-15`。Windows 的调度独立设置，
`quality-gate` 和发布工作流不随此变量切换。

```text
NARRANT_MACOS_RUNNER_LABELS=["self-hosted","macOS","ARM64","narrant-ci"]
```

## 本机部署

runner 使用专用的非管理员用户 `_narrantci`，工作目录为
`/Users/Shared/narrant-ci/runner/_work`，不复用日常开发 checkout。
工具、Cargo home、Rustup home 和缓存均在 `/Users/Shared/narrant-ci` 内。
Apple Command Line Tools 提供系统编译器与 SDK，无需为 runner 安装完整 Xcode。

系统 launchd 服务 `com.narrant.actions-runner` 使用官方 `runsvc.sh` 入口，
以 `_narrantci` 身份运行。配置为按需启动，不修改整机睡眠设置。
任务开始钩子仅在当前 Worker 存活期间阻止空闲睡眠。
启动、停止需要管理员权限：

```bash
sudo launchctl kickstart system/com.narrant.actions-runner
sudo launchctl kill SIGTERM system/com.narrant.actions-runner
```

runner 在线时可接收任务；退出、睡眠或断网时，指定此标签的任务会排队，
不会自动改用托管机器。runner 自动更新由官方程序管理；其他工具按需维护。

## 资源与清理

服务独立配置 `CARGO_BUILD_JOBS=1`、`CMAKE_BUILD_PARALLEL_LEVEL=1`、
`CARGO_INCREMENTAL=0`、`RUSTC_WRAPPER=sccache` 和 `SCCACHE_CACHE_SIZE=2G`。
不设置降低调试信息或改变优化等级的变量。
缓存目录为 `cache/sccache`，通过独立 `SCCACHE_SERVER_UDS` 避免连接开发用户的缓存服务。
自托管任务跳过整套 Rust target 的远端缓存，2 GiB 上限不包含工具链和编译期间的文件。

CI 设置 `NARRANT_CARGO_LOCK_FILE=/Users/Shared/narrant-ci/cargo.lock`。
本机开发仓库的 `.git/narrant-cargo-agent.lock` 指向同一文件，因此旧版和新版
`scripts/cargo-agent.sh`、linked worktree 与 CI clone 都能串行取得同一把锁。
各 checkout 仍使用自己的 target，本地开发继续保留 incremental。

本机 `hooks/job-started.sh` 和 `hooks/job-completed.sh` 通过官方 job hook 接入。
完成钩子只处理固定 CI checkout 中列出的生成目录，跳过符号链接与含已跟踪文件的目录，
保留日志；不触碰开发目录、个人文件和工具缓存。钩子位于 runner 安装目录之外。

完整语言验收需要为工具链、临时文件和系统交换空间保留余量；先运行轻量验收，
再按实际峰值安排完整任务。新安装不复制本地 target，也不清理仍在开发的 worktree。

## SDK 与验收

自托管任务通过 `xcrun` 发现本机 SDK，验证其声明可以被 Clang 读取，并把实际路径、
版本写入本次任务环境与日志。托管 `macos-15` 仍使用原有 SDK 15.5 配置。
在 macOS 27 上通过不代表已经验证 macOS 15；验收记录需保留实际系统与 SDK 版本。

手动运行 `Language support packs`，选择 `validation_scope=macos-runner`、
`platform=macos`、`upload_artifacts=false`、`publish=false`，只执行轻量接入检查：
真实服务身份、SDK、符号链接、2 GiB 缓存、共享 Cargo 锁，以及无依赖 Rust 库的编译与运行。
检查完成钩子后，还需确认生成物已回收而测试日志保留。
这不替代各语言的完整 macOS 验收，也不触发 `quality-gate`。

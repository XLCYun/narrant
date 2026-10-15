# macOS 自托管 CI

语言支持包、`quality-gate`、更新冒烟、macOS 打包、发布和证据汇总通过
`NARRANT_MACOS_RUNNER_LABELS` 选择 Apple Silicon runner。未设置变量时使用
下面的自托管标签，离线时排队，不自动切换到付费托管机器。原 Ubuntu 通用任务
改在 Mac 执行，不再产生 Linux 平台验证结果；产品目标仍为 macOS ARM64 和 Windows x64。

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
任务使用本地 pnpm store 和 sccache，不上传整套 Rust target 缓存；2 GiB 上限不包含工具链和编译期间的文件。

CI 设置 `NARRANT_CARGO_LOCK_FILE=/Users/Shared/narrant-ci/cargo.lock`。
本机开发仓库的 `.git/narrant-cargo-agent.lock` 指向同一文件，因此旧版和新版
`scripts/cargo-agent.sh`、linked worktree 与 CI clone 都能串行取得同一把锁。
各 checkout 仍使用自己的 target，本地开发继续保留 incremental。
自托管 Tauri build/dev 也通过 `scripts/run-tauri.mjs` 把内部 Cargo 交给同一入口，
签名命令不获取编译锁。

本机 `hooks/job-started.sh` 和 `hooks/job-completed.sh` 通过官方 job hook 接入。
完成钩子只处理固定 CI checkout 中列出的生成目录，跳过符号链接与含已跟踪文件的目录，
保留日志；不触碰开发目录、个人文件和工具缓存。钩子位于 runner 安装目录之外。

完整语言验收需要为工具链、临时文件和系统交换空间保留余量；先运行轻量验收，
再按实际峰值安排完整任务。新安装不复制本地 target，也不清理仍在开发的 worktree。

## SDK 与验收

自托管任务通过 `xcrun` 发现本机 SDK，验证其声明可以被 Clang 读取，并把实际路径、
版本写入本次任务环境与日志。
在 macOS 27 上通过不代表已经验证 macOS 15；验收记录需保留实际系统与 SDK 版本。

手动运行 `Language support packs`，选择 `validation_scope=macos-runner`、
`platform=macos`、`upload_artifacts=false`、`publish=false`，只执行轻量接入检查：
真实服务身份、SDK、符号链接、2 GiB 缓存、共享 Cargo 锁，以及无依赖 Rust 库的编译与运行。
检查完成钩子后，还需确认生成物已回收而测试日志保留。
这不替代各语言的完整 macOS 验收，也不触发 `quality-gate`。

## 桌面与发布任务

真实 Tauri E2E 和更新冒烟共用 `mac-m2-narrant` 及上面的
`NARRANT_MACOS_RUNNER_LABELS`。不新增虚拟机、独立系统或另一份 runner 注册。
当前 `_narrantci` 是不能交互登录的服务账户，LaunchDaemon 尚不能运行桌面验收；
调度修改不会自动改变主机上的启动方式，准备完成前桌面检查会明确失败。

同一 runner 的桌面启动需要一个已登录的 Aqua 用户会话。迁移时先确认 runner 空闲，
停止旧 LaunchDaemon，再把原安装目录的官方 `runsvc.sh` 配置为该用户的 LaunchAgent；
保留注册、标签、独立 checkout、工具缓存、完成钩子与共享 Cargo 锁。
账户变化时先备份配置并调整相关目录权限，不能同时启动后台和桌面两个 Listener。
`scripts/verify-desktop-runner.mjs` 会在大型构建前检查会话，真实 E2E 仍需完整运行。

更新冒烟会安装应用、临时修改系统 hosts 和证书信任，并启动本地 HTTPS 服务，
需要执行这些系统操作的权限。它在当前机器运行，不要求额外的隔离系统声明。
保持已有 hosts 备份、临时证书删除、进程退出及恢复校验；失败时保留恢复所需文件。
每台机器只运行一个 runner，桌面任务依次执行。测试期间避免同时手动运行 Narrant；
若所选登录账户已有 Narrant 数据，现有保护会拒绝冒烟，不得删除个人数据来让测试通过。
先解决登录账户、测试数据目录和系统权限，再运行真实更新验收。

普通发布任务不需要上述系统权限。macOS 签名使用本次任务的临时钥匙串和公证密钥，
成功或失败后都恢复原钥匙串列表并删除凭据。两条 OSS 发布流程共用
`scripts/install-ci-ossutil.sh`，校验后安装官方 Apple Silicon ossutil 1.7.19，保留
OIDC、受保护 Environment、签名和真实更新证据校验。迁移 runner 不构成发布授权。

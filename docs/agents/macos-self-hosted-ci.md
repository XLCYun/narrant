# macOS 自托管 CI

语言支持包、`quality-gate`、更新冒烟、macOS 打包、发布和证据汇总通过
`NARRANT_MACOS_RUNNER_LABELS` 选择 Apple Silicon runner。未设置变量时使用
下面的自托管标签，离线时排队，不自动切换到付费托管机器。原 Ubuntu 通用任务
改在 Mac 执行，不再产生 Linux 平台验证结果；产品目标仍为 macOS ARM64 和 Windows x64。

```text
NARRANT_MACOS_RUNNER_LABELS=["self-hosted","macOS","ARM64","narrant-ci"]
```

## 本机部署

runner 复用当前已登录的 `lizhanfa` 桌面账户，工作目录为
`/Users/Shared/narrant-ci/runner/_work`，不复用日常开发 checkout。
CI 的 HOME、工具、Cargo home、Rustup home 和缓存均在 `/Users/Shared/narrant-ci` 内；
独立 HOME 用于隔离工具配置，不代表新增系统账户或登录会话。
Apple Command Line Tools 提供系统编译器与 SDK，无需为 runner 安装完整 Xcode。
C/C++ 验收所需的 CMake 由 `provision-native-toolchains.mjs` 按清单下载、校验并解包到任务临时目录，
通过 `NARRANT_CMAKE` 传给测试；不要求全局安装 CMake，也不将它打入用户支持包。

用户 LaunchAgent `com.narrant.actions-runner` 使用官方 `runsvc.sh` 入口，
位于 `~/Library/LaunchAgents/com.narrant.actions-runner.plist`，只在已有 Aqua 会话中运行。
配置为按需启动，不启用自动登录，不修改整机睡眠设置。
任务开始钩子仅在当前 Worker 存活期间阻止空闲睡眠。
这不能阻止合盖或手动睡眠；运行任务时保持机器开盖、联网。若因此断联，先确认 runner
恢复在线，再只重跑受影响的 job，不将未执行的测试记为通过。
在该账户已登录时启动、停止：

```bash
launchctl kickstart gui/$(id -u)/com.narrant.actions-runner
launchctl kill SIGTERM gui/$(id -u)/com.narrant.actions-runner
```

2026-10-15 已停用旧 `_narrantci` LaunchDaemon，保留同一 runner 注册 ID 22。
原服务配置与权限迁移记录保存在 root 专属目录
`/Users/Shared/narrant-ci/desktop-migration-20261015`。回滚前先停止用户 Listener，
再按备份恢复服务配置和 CI 目录权限；两个启动方式不得同时运行。

runner 在线时可接收任务；账户退出登录、睡眠或断网时，指定此标签的任务会排队，
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

开始钩子记录实际剩余空间，不再用固定容量门槛拒绝任务。完整语言验收需要为工具链、
临时文件和系统交换空间保留余量；按任务范围和实际峰值安排验证。新安装不复制本地 target，
也不擅自清理仍在开发的 worktree；确需提前释放其编译产物时，先取得用户授权。

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

真实 Tauri E2E 共用 `mac-m2-narrant` 及上面的 `NARRANT_MACOS_RUNNER_LABELS`。
测试应用使用 `tauri.e2e.conf.json` 中的独立身份、无痕 WebView、临时数据库与仓库，
凭据保存在内存，并禁用正式应用更新。即使漏传 E2E 配置，测试构建也会在初始化用户目录前停止。
真实应用测试会在当前桌面打开测试窗口，不能据此关闭日常 Narrant 或删除个人数据。
`scripts/verify-desktop-runner.mjs` 会在大型构建前检查会话，真实 E2E 仍需完整运行。
`test:e2e` 仅为 WDIO 进程设置 `NODE_USE_ENV_PROXY=0`，避免 Node 24 的全局代理 dispatcher
与 WDIO 自带的 Undici 混用后拒绝 `Content-Length`。下载、构建仍沿用 runner 的代理配置；
WDIO 本身继续按 `HTTP_PROXY`/`NO_PROXY` 处理请求，本机驱动不经过代理。

升级验收使用独立 `Narrant Upgrade Test`，identifier 为 `com.cyunlab.narrant.upgrade-test`，
可执行文件名为 `narrant-upgrade-test`。旧、新测试包分别取上一 Stable 和候选源码，仅叠加隔离身份、
内存凭据和数据观察入口。TLS 通过测试进程自己的代理与 CA 文件工作，不修改 hosts、系统信任或代理。
正式包另做静态签名、完整性与信任根核验；测试包升级不等同于正式产品身份下实际升级。

`update-smoke.yml` 的 `isolated-upgrade` 产生发布验收，`preflight` 只产生不可发布的分支验收。
继续保留 `NARRANT_CI_SHARED_DESKTOP=1` 和 checkout 外 hook 对旧 `native-smoke` job 的阻止，
防止重跑历史工作流误用日常安装。新入口使用 `verify-desktop-runner.mjs --isolated-update-test`，
不要求 sudo、管理员或 hosts 写权限。缺少独立 profile 或发现旧测试安装/数据时停止，保留现场。
失败后只清理本次拥有的测试进程、安装和数据，不能清理日常 Narrant。

普通发布任务不需要上述系统权限。macOS 签名使用本次任务的临时钥匙串和公证密钥，
成功或失败后都恢复原钥匙串列表并删除凭据。两条 OSS 发布流程共用
`scripts/install-ci-ossutil.sh`，校验后安装官方 Apple Silicon ossutil 1.7.19，保留
OIDC、受保护 Environment、签名和真实更新证据校验。迁移 runner 不构成发布授权。

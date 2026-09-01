# Task 3 Report: Trusted Tauri and platform adapters

## Result

Task 3 已完成。实现提交为 `7a67c6f`（`feat: add trusted application update adapters`）。
本报告作为随后独立的交付记录提交，不包含 Task 4 UI 或 Task 5/6 发布 workflow/smoke。

## Files

- 依赖与构建配置：
  - `src-tauri/Cargo.toml`
  - `src-tauri/Cargo.lock`
  - `src-tauri/build.rs`
- Tauri updater/bundle/platform 配置：
  - `src-tauri/tauri.conf.json`
  - `src-tauri/tauri.macos.conf.json`
  - `src-tauri/tauri.windows.conf.json`
  - `src-tauri/tauri.e2e.conf.json`
  - `src-tauri/gen/schemas/acl-manifests.json`
  - `src-tauri/gen/schemas/desktop-schema.json`
  - `src-tauri/gen/schemas/macOS-schema.json`
- Application Update adapters：
  - `src-tauri/src/application_update/config.rs`
  - `src-tauri/src/application_update/verifier.rs`
  - `src-tauri/src/application_update/diagnostics.rs`
  - `src-tauri/src/application_update/platform.rs`
  - `src-tauri/src/application_update/runtime.rs`
  - `src-tauri/src/application_update/mod.rs`
  - `src-tauri/src/application_update/state.rs`（仅增加 test-only operation identity fixture）
- Tauri composition/commands/shutdown：
  - `src-tauri/src/tauri_app/bootstrap.rs`
  - `src-tauri/src/tauri_app/commands/application_update.rs`
  - `src-tauri/src/tauri_app/commands/mod.rs`
  - `src-tauri/src/tauri_app/mod.rs`
  - `src-tauri/src/tauri_app/shutdown.rs`
- 独立测试：
  - `src-tauri/src/application_update/tests/config.rs`
  - `src-tauri/src/application_update/tests/verifier.rs`
  - `src-tauri/src/application_update/tests/diagnostics.rs`
  - `src-tauri/src/application_update/tests/platform.rs`
  - `src-tauri/src/application_update/tests/runtime.rs`
  - `src-tauri/src/tauri_app/tests/shutdown.rs`

## Requirement mapping

1. **依赖、rustls 与 shipping targets**
   - 固定兼容版本 `tauri-plugin-updater = =2.10.1` 并启用 rustls。
   - reqwest 使用 rustls + streaming；加入 minisign、tar/gzip、tempfile 与取消 token 所需最小依赖。
   - base bundle 只声明 `nsis`/`dmg`，平台 overlay 分别收敛为 DMG 与 current-user NSIS；`createUpdaterArtifacts` 开启，E2E overlay 明确关闭制品生成。

2. **编译期 trust 配置 fail closed**
   - endpoint 必须由编译期 `NARRANT_UPDATER_ENDPOINT` 提供且逐字等于
     `https://github.com/cyunlab/narrant/releases/latest/download/latest.json`。
   - `NARRANT_UPDATER_PUBLIC_KEY` 必须是可解码、可解析的 Tauri 外层 base64 minisign 公钥；缺失/无效不会创建 staging/verifier，检查进入不可重试 manifest failure。
   - updater plugin Builder 与每次 custom `updater_builder()` 使用同一编译期公钥；plugin config 和 custom Builder 使用同一固定 endpoint。custom Builder 固定当前 target、30 秒 manifest timeout，并对解析后的 manifest 施加 64 KiB 上限。
   - `build.rs` 跟踪两个编译期变量。源码/config 没有真实公钥或私钥；`tauri.conf.json` 的空 `pubkey` 只由 Rust plugin Builder 在有效构建输入时覆盖，缺少输入保持 fail closed。

3. **可信候选与下载**
   - Rust 只保存 Tauri trusted Builder 返回的 `Update`，并把它与精确 `UpdateOperationId`、版本、URL、签名绑定。
   - typed commands 不接受 URL、文件路径、签名或 target；retry 只重放 core 已记录的 operation。
   - 初始包 URL 必须是 `github.com/cyunlab/narrant/releases/download/...` HTTPS；最多五次重定向且只允许 GitHub HTTPS Release 资产主机。
   - 下载流写临时文件、报告已知/未知总量进度、响应取消 token、`sync_all` 后再交给 staging。

4. **真实验证、durability 与安装结果**
   - `DiskMinisignVerifier` 对真实磁盘文件做固定缓冲的 prehashed minisign 流式验证，不把整个包读入内存。
   - `StageCommitOutcome::Committed` 直接携带 opaque durable proof；`DurabilityUnknown` 必须先成功 `confirm_durability`，否则绝不提交 `DownloadSucceeded`。
   - 安装继续使用 Task 2 `install_with`：在 staging lease 内复验，保留 `Installed { cleanup }` 与 `HandedOff` 的既有 cleanup/reconciliation 语义。

5. **平台语义**
   - 仅接受 `darwin-aarch64 + MacosAppArchive` 和 `windows-x86_64 + WindowsNsis`。
   - macOS 将唯一 `.app` 解包到目标同文件系统临时目录，先移动原 app，再移动候选；候选移动或 post-replace 失败会恢复原路径，回滚本身失败时保留恢复副本。系统命令使用结构化 argv，没有 caller shell 拼接。
   - Windows 只接受磁盘复验后的 PE/EXE，复制到独立临时目录并使用结构化 `Command::args` 启动 current-user NSIS；普通退出参数为 `/P /UPDATE`，显式重启为 `/P /UPDATE /R`。安全 spawn 后返回 `HandedOff`，由候选进程版本 reconciliation 清理 staging。

6. **Tauri state/event/commands/scheduler**
   - `ApplicationUpdateRuntime` 作为 `Arc` managed state 装配；启动发出 `Ready`，core 继续负责启动检查、六小时周期、自动下载、1/2/4 秒 retry、coalescing 与 sequence。
   - 仅发布 `application-update:snapshot` 一个完整快照事件。
   - typed commands：snapshot、manual check、automatic-download preference、retry、restart。
   - 下载、进度、完成与失败全程穿透原始 operation identity；关闭自动下载会取消精确下载 token，迟到结果仍由 core identity gate 拒绝。

7. **共享 stop 与安装顺序**
   - 普通退出先等待 Task 1 `ApplicationShutdown::stop_once`；只有 `Completed` 才检查 durable staging 并安装，`Failed/TimedOut` 跳过安装但沿既有 final exit 退出。
   - 显式 restart 使用同一个 stop outcome；安装请求有进程内 once gate，防止重复点击造成双安装。
   - macOS 只有 `Installed` 后才 `app.restart()`；Windows 只有 installer 安全 handoff 后才 final exit，并由 `/R` 让 installer 接管 relaunch。安装失败会退出已完成 stop 的旧进程而不会伪装成 relaunch 成功。
   - Application Update 没有复制 Babysit、Review、telemetry 或 provider-specific cleanup。

8. **诊断与 capability boundary**
   - JSON Lines 诊断只保存稳定 event/category/version/sequence/operation ID，不保存签名、headers、credentials、Release Notes、URL 或用户路径。
   - updater Rust plugin 已进入正常 Tauri schema；`capabilities/default.json` 没有授予 updater JS permission，且没有安装 `@tauri-apps/plugin-updater`，前端仍只能使用窄 typed commands。

## Verification

- `scripts/cargo-agent.sh test --lib application_update --manifest-path src-tauri/Cargo.toml`
  - **PASS**：53 passed, 0 failed；覆盖 config fail closed、HTTPS allowlist、manifest 上限、真实磁盘签名/篡改、诊断脱敏、target/install-kind、macOS rollback、Windows argv，以及全部 Task 2 core/staging 契约。
- `scripts/cargo-agent.sh test --lib tauri_app::shutdown --manifest-path src-tauri/Cargo.toml`
  - **PASS**：9 passed, 0 failed；包括 cleanup-before-install、timeout/failed skip install、15 秒 deadline 与共享 outcome。
- `scripts/cargo-agent.sh check --lib --manifest-path src-tauri/Cargo.toml`
  - **PASS**：无 warning。
- `scripts/cargo-agent.sh fmt --manifest-path src-tauri/Cargo.toml -- --check`
  - **PASS**。
- `git diff --check`
  - **PASS**。
- `pnpm tauri info`
  - **PASS**：Tauri 正常解析应用配置并识别 Rust updater `2.10.1`；同时确认 updater JS package 未安装。命令报告宿主 Node 26 超出项目 engine 以及未安装完整 Xcode，但不影响本次配置解析。
- Tauri schemas
  - **PASS**：通过 `tauri-build`/Cargo 的仓库正常 Tauri tooling 自动生成，未手工伪造 schema；default capability 未增加 updater JS 权限。
- `scripts/cargo-agent.sh check --lib --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`
  - **ENVIRONMENT BLOCKED**：macOS 宿主缺 Windows SDK/MSVC headers，`aws-lc-sys` 在进入 Narrant crate 前因 `windows.h`/`stdlib.h` 缺失失败。Windows adapter 因此由跨平台 contract tests 覆盖，真实 Windows 编译/安装留给 Windows runner。

未运行全量 Rust suite，也未触发 quality-gate，符合 Task 3/Cargo 验证节奏。

## Residual risks / required release inputs

- 正式 bundle 必须同时设置：
  - `NARRANT_UPDATER_ENDPOINT` 为上述固定 production URL；
  - `NARRANT_UPDATER_PUBLIC_KEY` 为 Narrant updater minisign 公钥的 Tauri 外层 base64 文本；
  - updater 制品签名所需私钥/password 由 Task 5 的受保护 release environment 提供。
- 本提交没有真实私钥、冒充 production 的公钥或 GitHub secret/environment 修改。
- 本机没有执行真实 Windows NSIS 生命周期，也没有执行“替换已安装 DMG 应用”的原生升级；这些是 Task 6 Windows/macOS runner native smoke 的职责。
- Tauri Builder 本身会先解析网络响应；本实现随后拒绝序列化超过 64 KiB 的 manifest，并以 30 秒 timeout 限制请求。若未来要求在分配 JSON DOM 之前强制 byte-level hard cap，需要在 Tauri updater 上游提供 bounded body hook 或换成保持同等 target/signature 语义的受审 metadata parser。
- macOS 直接替换要求当前 app 所在目录可写；权限不足会安全失败并保留 staging，不使用 shell 拼接或隐式提权。若产品最终要求管理员授权安装，需要另行引入经过审查的原生授权 helper，并补真实签名/公证 smoke。

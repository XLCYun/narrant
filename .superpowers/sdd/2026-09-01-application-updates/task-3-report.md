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

## Fix round 1（独立审查 9 Important + 2 Minor）

本节取代上文关于“由 Tauri Builder 先解析 manifest”“下载 token map”和“分离退出 gate”的旧实现描述。修复实现提交为 `f1706a1`（`fix: harden application update lifecycle`）。

### Finding-by-finding 修复

1. **Manifest byte hard cap / redirect policy**
   - 新增 `application_update/metadata.rs`。rustls reqwest 在创建 JSON DOM 前逐块读取，Content-Length 与 chunked/stream 两条路径都执行 64 KiB hard cap，并使用 30 秒总 timeout。
   - production 初始 endpoint 仍由 `TrustedUpdaterConfig` 逐字固定；manifest redirect 只允许无 credentials、无显式 port 的 HTTPS `github.com` 固定 manifest path，或 GitHub release asset 主机的非空 asset path，最多五跳。
   - adapter 直接反序列化 `tauri-plugin-updater 2.10.1` 的公开 `RemoteRelease`，因此保留 Tauri stable manifest 的 dynamic/static target、version/notes、URL/signature wire semantics；随后仍按固定 target/install-kind 与 package allowlist 构造 private `StableRelease`。下载和磁盘 minisign 验证仍使用同一编译期 trust config。

2. **真实 status/错误分类**
   - 只有 transport、timeout/408、429、5xx 分别进入 Network、RequestTimeout、RateLimited、Server 并允许 retry。
   - 400/404、redirect contract、oversize、malformed JSON/SemVer、missing target、empty signature、package/target contract 全部进入 Manifest 且不可 retry。
   - 明确 HTTP status 作为可选数值写入脱敏 diagnostics；不保存响应正文或 URL。

3. **Managed updater task owner / shared stop**
   - periodic timer、retry timer、check 和 download 全部注册到 Task 1 的单一 `CancellableOperations`。`CancellationSignal` 增加 enable-before-check 的异步唤醒；`close_and_drain` 关闭 admission、取消所有 updater tasks 并等待 RAII registration 归零。
   - download future 的 registration 覆盖 stream EOF、磁盘 sync、真实 minisign verification、staging 文件锁与 durable commit 临界区。同步临界区超出 15 秒时共享 `ApplicationShutdown` 返回 TimedOut，post-stop coordinator 使用 `try_lock` 跳过安装并 final exit，不会在 deadline 外等待 staging mutex。
   - timer identity 使用单调序号，避免前一个 timer guard 尚未释放时后继 schedule 因 duplicate ID 丢失；关闭 admission 后不会执行新的网络检查。

4. **取消与 durable side effect 原子线性化**
   - 下载 EOF 后先检查 cancellation，再进入 runtime `transition_gate`。在同一 gate 内，先由 core `accepts_download_completion(operation_id)` 核对 active identity，再持 staging/root lock 执行 stage、必要的 pending durability confirmation，最后直接提交同一 identity 的 `DownloadSucceeded`。
   - `SetAutomaticDownload(false)` 经过同一 transition gate：若偏好取消先获得 gate，active identity 消失且磁盘不写；若 stage 先获得 gate，opaque durable proof 与 core acceptance 已在同一受控边界完成，之后偏好变更保留合法 Staged 状态。旧 staging 仍由 Task 2 generation/pointer 事务保护。
   - download token map 已删除；取消、正常返回、错误与 panic unwind 均由 operation registration RAII 统一清理。

5. **stop outcome 后的 final exit**
   - `Completed` 才尝试安装；`Failed`、`TimedOut` 都跳过安装但仍执行一次 final exit。安装失败发生在不可逆 stop 后会保留 staging、写脱敏 warning，并以非零 code final exit。

6. **进程级单一 post-stop/install/final-action coordinator**
   - 新增 managed `PostStopCoordinator`。普通 ExitRequested/CloseRequested 与显式 update restart 共享同一 CAS execution gate，UpdateRestart intent 可在 stop 期间覆盖普通退出 intent。
   - 唯一 owner 复用 Task 1 `stop_once` outcome，installer 至多一次；macOS `Installed` 的显式 restart 才调用一次 `app.restart()`，Windows `HandedOff` 只 final exit、由唯一 `/R` installer 接管 relaunch。普通退出与安装失败也只产生一次 final action。

7. **macOS real archive/FS safety**
   - tar.gz 改为逐 entry 审查与解包：拒绝 absolute/`..`、duplicate path、symlink、hardlink 和特殊文件；只允许唯一 `.app` root 下的 directory/regular file。解包后使用 `symlink_metadata` 要求 root 本身为真实目录。
   - candidate move 失败直接恢复 backup；post-replace 失败先将 destination 移到 `failed-candidate.app`，再把 backup 恢复到原路径。第二步失败时 temp work 通过 `keep()` 保留 backup/failed candidate 等恢复材料。

8. **Runtime recovery visibility**
   - runtime 装配不再对 install plan、context、verifier、repository open/recovery 使用 `.ok()` 静默丢失。
   - “repository 可用且损坏候选已安全丢弃”会保留 repository、发布 Verification/Storage startup failure snapshot 和脱敏 diagnostics，随后仍允许 Ready/check/download；“repository 根不可打开”发布 Storage failure 且保持 staging unavailable，检查仍可恢复运行并明确在下载提交时报 Storage。

9. **真实组合与平台 contract tests**
   - 真实 loopback HTTP server 覆盖 chunked hard cap、redirect allow/deny、timeout、404/408/429/503、malformed JSON/SemVer、empty signature 与 missing target。
   - 真实 staging filesystem 覆盖 EOF cancel-before-commit 与 stage-wins 的线性化、startup tamper recovery、repository unavailable。
   - macOS 使用真实 gzip tar 和临时目录覆盖 candidate move、post-replace rollback、root symlink、hardlink、path traversal。
   - Windows 共享核心在 macOS 可编译测试：可注入 `CommandLauncher` 验证复制后的 temp lifetime、DOS `e_lfanew`、`PE\0\0`、AMD64 machine `0x8664`、结构化 argv 与一次 launch。静态代码无法可靠证明任意 PE 为 NSIS，因此边界是“已经 minisign 验证的发布制品 + x64 PE contract + 固定 NSIS argv”；真实 NSIS 行为继续由 Windows runner 验证。

10. **Minor findings**
    - 删除易泄漏的 download-token HashMap，改由 shared operation registration RAII 负责所有出口清理。
    - `SafeDiagnostic` 已添加中文结构说明，并只增加安全的数值 `httpStatus`。

### Fix round files

- 新增：
  - `src-tauri/src/application_update/metadata.rs`
  - `src-tauri/src/application_update/tests/metadata.rs`
- 主要修改：
  - `src-tauri/src/application_update/{config,diagnostics,platform,runtime,state,staging}.rs`
  - `src-tauri/src/application_update/tests/{config,platform,runtime,staging}.rs`
  - `src-tauri/src/operation/{cancellable.rs,cancellable_tests.rs}`
  - `src-tauri/src/tauri_app/{bootstrap,shutdown}.rs`
  - `src-tauri/src/tauri_app/tests/shutdown.rs`

### Fix round verification

- `scripts/cargo-agent.sh test application_update --manifest-path src-tauri/Cargo.toml`
  - **PASS**：63 passed, 0 failed（包含真实 HTTP、minisign disk、staging FS、mac tar/FS、Windows PE/launcher contract）。
- `scripts/cargo-agent.sh test tauri_app::shutdown::tests --manifest-path src-tauri/Cargo.toml`
  - **PASS**：11 passed, 0 failed（包含 15 秒 timeout、Failed/TimedOut skip install + final action、普通 exit/restart intent race）。
- `scripts/cargo-agent.sh test operation::cancellable::tests --manifest-path src-tauri/Cargo.toml`
  - **PASS**：9 passed, 0 failed（包含 async cancel wake、drain 与 admission close）。
- `scripts/cargo-agent.sh check --lib --manifest-path src-tauri/Cargo.toml`
  - **PASS**。
- `scripts/cargo-agent.sh fmt --manifest-path src-tauri/Cargo.toml -- --check`
  - **PASS**（最终提交前复核）。
- `git diff --check`
  - **PASS**（最终提交前复核）。
- `pnpm tauri info`
  - **PASS（配置解析）**：识别 Rust `tauri-plugin-updater 2.10.1`，且 `@tauri-apps/plugin-updater` 未安装；宿主仍提示 Node 26 超出项目 engine 且未安装完整 Xcode，不影响本轮配置/schema 解析。
- `scripts/cargo-agent.sh clippy --lib --manifest-path src-tauri/Cargo.toml -- -D warnings`
  - **BASELINE BLOCKED**：本轮已修复 Task 2 staging lock 的 `suspicious_open_options`，但仓库既有 Babysit `result_large_err` warnings 仍使全 lib `-D warnings` 失败；未修改不相关 Babysit。

未运行全量 Rust suite，未触发 quality-gate。

### Fix round residual risks

- Windows 本机仍缺 MSVC/Windows SDK，真实 NSIS current-user install、进程退出后 `/R` relaunch 与签名制品行为必须由 Windows runner 覆盖；本轮共享 PE/argv/launcher contract 已在 macOS 执行。
- macOS 测试真实 tar 与文件系统事务，但没有替换 `/Applications` 中已签名/公证的 production app；原生 smoke 仍属于 Task 6。
- build-time endpoint/public key/release private key 要求不变；本轮没有新增或提交任何真实/冒充 production 的密钥，也没有修改 GitHub secrets/environments。
- 本轮没有修改 Tauri config/capability schema、Task 4 UI 或 Task 5/6 workflow/smoke。

## Fix round 2（Round 1 scoped re-review 的 4 个 PARTIAL）

Round 2 实现提交为 `81d8c52`（`fix: close application update integration gaps`）。本节取代 round 1 对顺序 helper 测试的覆盖表述；只有下列 production-shared executor/seam 覆盖才视为 runtime/post-stop 组合证据。

### 1. Production GitHub manifest redirect chain

- 初始 endpoint 仍必须逐字等于 `PRODUCTION_UPDATE_ENDPOINT`。
- 同一 pure production policy 现在额外允许：
  - `https://github.com/cyunlab/narrant/releases/download/<tag>/latest.json`；
  - `<tag>` 必须为 `v` 可选前缀的 stable SemVer，非空，不是 `.`/`..`，不能包含 `/`、percent encoding 或非安全字符；
  - GitHub 中间 URL 必须固定 owner/repo/filename、无 credentials/query/fragment/显式 port；
  - 后续只允许既有 GitHub asset hosts 的 HTTPS 非空 asset path、无 credentials/fragment/显式 port。asset host 允许 GitHub 实际下载链需要的签名 query；query 不会改变固定 host/path trust owner。
- production-policy contract test 对 `latest -> v1.1.5 tag-specific -> release-assets.githubusercontent.com` 三段 URL 使用与生产 redirect callback 完全相同的 `trusted_manifest_url`，并覆盖其他 owner/repo、空/`.`/`..`/encoded tag、错误 filename、query/fragment/port 拒绝。loopback fixture policy 没有被放宽。
- 2026-09-02 只读核对：固定 latest endpoint 返回 `302` 到 `.../releases/download/v1.1.5/latest.json`，新 policy 接受该首跳；当前 tag-specific URL 随后返回 `404`，说明当前 GitHub Release 尚未提供 `latest.json` asset。代码已不再误拒合法首跳，但正式检查成功仍要求 release workflow 上传 manifest。

### 2. Stable metadata/controller semantics

- HTTP 204 在读取/解析 body 前直接映射为 `Ok(None)`，与 Tauri 2.10.1 stable check 的 no-update 语义一致。
- `RemoteRelease` 的非空 prerelease 在构造 `StableRelease` 前映射为不可 retry 的 `Manifest`。
- controller 在清除 active identity、retry/failure 前先完成候选 SemVer/stable-channel 验证；未来 adapter 漏检时不会产生“active 已清除但 snapshot 仍 checking”的半转换。
- runtime 的共用 `transition` 捕获 `UpdateDomainError`，写脱敏 diagnostics，再提交有序 `RuntimeDomainFailed` snapshot；下载中的 domain failure 同时产生精确 CancelDownload effect，不再静默 return。
- 真实 loopback adapter + controller 组合测试覆盖 prerelease 最终 `Failed/Manifest`、active 收敛后可开始新 check，以及 204 最终 `UpToDate`。
- 真实 runtime test 额外故意注入漏过 adapter 的 prerelease `StableRelease`，验证 sequence 增长、最终 `Failed/Manifest`、新 check 可进入、diagnostics 不含版本文本。

### 3. Queryable startup recovery warning

- `UpdateSnapshot` 新增独立可选 `recoveryWarning` provider-neutral 投影，只含稳定 `category/message`；它可以与 `Checking`/后继状态同时存在。
- startup 损坏候选/仓储不可打开仍先写 diagnostics；bootstrap 随后的 `Ready` 只启动检查，不覆盖 warning。snapshot command 与完整 snapshot event 都会携带 warning。
- warning 在同一进程内保持可查询；成功提交新的 opaque durable proof/`DownloadSucceeded` 后明确清除。重启时仓储恢复成功也自然不再创建 warning。
- production 与测试共用 `ApplicationUpdateRuntime::new_at_paths` 和 `transition`。真实 filesystem tests 分别构造签名 generation 后篡改 package、以及把 cache root 变为普通文件；随后创建 runtime、dispatch Ready、读取 serialized snapshot 与 diagnostics。两者最终均为 `Checking + recoveryWarning`，分类分别是 Verification/Storage，且 diagnostics/snapshot 不含路径或签名；仓储可用场景仍产生真实 `CheckStable` effect。

### 4. Production-shared combination seams

#### Download runtime

- production `download_pending` 与测试共用 `consume_stream_and_commit` / `commit_download_file`：真实 stream、EOF、temporary disk sync、StageCandidate、DiskMinisignVerifier、StagingRepository、pending durability confirmation、transition gate、core identity/proof commit 全部在同一执行路径。
- transition gate 内二次检查 shared cancellation，覆盖“EOF 后等待 gate 时才取消”的窗口。
- 三个受管任务测试使用真实 `CancellableOperations::run`：
  - EOF 前取消：stream 保持打开，收到 progress 后 preference + registry cancel；磁盘无 proof，core Available，registration 释放且同 ID 可复用；
  - EOF 后/gate 前取消：测试持有真实 transition mutex，before-commit hook 证明任务已过 EOF，再 cancel/release；二次 cancellation check 阻止 stage，磁盘/core 未 staged，RAII 可复用；
  - stage 先赢：真实 minisign `test` bytes 验证并提交，磁盘 proof 与 core 均 Staged，registration 释放。

#### Post-stop coordinator

- production `PostStopCoordinator::run` 与测试共用 `execute_post_stop`，注入 stop Future、installer closure 与唯一 final-action closure；测试不再手工增加虚构计数。
- outcome matrix 真正执行 Completed/Failed/TimedOut、install error、no staged、macOS `Installed`、Windows `HandedOff`、ordinary install：
  - 只有 Completed 调 installer；
  - install error -> Exit(1)；Failed/TimedOut/HandedOff/no staged -> Exit(0)；
  - 只有 UpdateRestart + Installed -> Restart；
  - 每例 final action 恰好一次，停止后的进程不存在无 final action 分支。
- Exit/Close/UpdateRestart 使用多个线程竞争同一个 coordinator begin gate，stop completion 前 restart intent 成功升级；production-shared executor 最终只调用一次 installer 与一次 Restart。
- runtime 的 `try_use_staged` 与 production install 共用 `Mutex::try_lock`；真实 runtime 持有 staging mutex 时测试证明立即返回错误（<50ms），不会在 15 秒 stop deadline 外等待。

### Round 2 focused verification

- `scripts/cargo-agent.sh test application_update --manifest-path src-tauri/Cargo.toml`
  - **PASS**：71 passed, 0 failed。
- `scripts/cargo-agent.sh test application_update::runtime::tests --manifest-path src-tauri/Cargo.toml`
  - **PASS**：8 passed, 0 failed。
- `scripts/cargo-agent.sh test application_update::metadata::tests --manifest-path src-tauri/Cargo.toml`
  - **PASS**：4 passed, 0 failed。
- `scripts/cargo-agent.sh test application_update::tests::config --manifest-path src-tauri/Cargo.toml`
  - **PASS**：4 passed, 0 failed。
- `scripts/cargo-agent.sh test tauri_app::shutdown::tests --manifest-path src-tauri/Cargo.toml`
  - **PASS**：13 passed, 0 failed。
- `scripts/cargo-agent.sh check --lib --manifest-path src-tauri/Cargo.toml`
  - **PASS**。
- `scripts/cargo-agent.sh fmt --manifest-path src-tauri/Cargo.toml -- --check`
  - **PASS**。
- `git diff --check`
  - **PASS**。

未运行全量 Rust suite，未触发 quality-gate。

### Round 2 residual risks / report scope

- 当前 GitHub tag `v1.1.5` 下的 `latest.json` 只读请求为 404；release pipeline 在正式启用更新前必须上传与固定 endpoint/tag chain 匹配的 stable manifest。这不是客户端 allowlist 放宽可以解决的问题。
- Windows production `SystemCommandLauncher`/target cfg 仍未在 macOS 编译，真实 NSIS current-user lifecycle 与 `/R` relaunch 必须由 Windows runner/Task 6 覆盖。Round 2 只声称 production-shared post-stop outcome 与既有 PE/temp/argv launcher core contract 覆盖，不声称 native Windows adapter 已在本机证明。
- macOS production app replacement/native restart 仍需签名、公证 runner smoke；本轮没有修改 UI、workflow、secrets、capability schema 或制品发布状态。

## Fix round 3（Round 2 re-review 的 1 个 Important）

Round 3 实现提交为 `004ee9f`（`fix(updater): preserve staged update on preference errors`）。本节修正 round 2 对所有 `UpdateDomainError` 使用同一破坏性收敛输入的设计；metadata/check/download contract failure 的既有安全收敛保持不变。

### 1. 偏好持久化失败与更新包真相分离

- `UpdateSnapshot` 新增可选 `preferenceWarning`，只公开稳定 `category/message`，与 `state`、durable `candidate`、operation `failure`、`recoveryWarning` 可以同时存在；不会序列化原始存储错误或路径。
- runtime 不再把 `PreferenceError` 发送给通用 domain-failure 分支。偏好保存失败改为专用 `PreferenceSaveFailed` 输入，只设置/替换 warning 并递增一次 sequence，不改变 state、candidate、failure、active check/download identity 或 retry。
- 非偏好领域错误继续使用 `ContractDomainFailed`：它仍结束对应非法领域转换、取消下载（如适用）并生成有序 `Failed`，因此 round 2 的 prerelease/contract fail-closed 行为没有放宽。

### 2. rollback、sequence 与 restart 语义

- `SetAutomaticDownload` 仍先执行原子存储，再改变内存偏好。save 失败时原输入没有被 controller 接受、sequence 不增加，内存 `automaticDownload` 保持磁盘原值；随后 runtime 的专用 warning 输入被接受并只增加一个 sequence。
- save 成功后才改变内存偏好，并明确清除旧 `preferenceWarning`。关闭自动下载的 cancel/retry 失效逻辑仍只发生在保存成功后，因此 UI 不会显示尚未落盘的值，也不会因失败误取消下载。
- 已有 opaque durable proof 时，保存失败后公开投影仍为 `Staged`，candidate/proof 不变；`RequestUpdateRestart` 继续产生唯一 `RequestInstall { UpdateRestart }` effect。
- 没有 staged candidate 时，原有 Idle/Checking/Downloading 等状态保持不变，warning 仍通过 snapshot command/单一 event 可见且可诊断。

### 3. 真实 failing-store 与活动 operation 覆盖

- controller 注入 `FailingPreferences`，覆盖：
  - staged + save failure 仍 Staged、偏好保持磁盘真值、warning 为 Storage、restart 仍可安装；
  - no-staged + save failure 只增加一次 sequence、保持 Idle，并验证序列化 warning 不含私有路径；
  - active check + save failure 后，原 operation ID 的 `CheckSucceeded` 仍被接受；
  - active download + save failure 后，原 operation ID 的 progress 仍被接受，且没有 CancelDownload effect。
- runtime 使用真实 `FilePreferenceStore` 与临时文件系统：把正式偏好路径构造成目录，使原子替换确定性失败；验证 snapshot 保持默认磁盘真值、`Idle + preferenceWarning(Storage)`，并验证 snapshot/JSONL diagnostics 不含数据目录或偏好文件名。

### Round 3 focused verification

- `scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml application_update::tests::state_machine::preference_save_failure -- --nocapture`
  - **PASS**：4 passed, 0 failed。
- `scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml application_update::runtime::tests::runtime_preference_save_failure -- --nocapture`
  - **PASS**：1 passed, 0 failed。
- `scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml application_update:: -- --nocapture`
  - **PASS**：76 passed, 0 failed；包含 round 1/2 的 HTTP、redirect、真实 minisign/staging、平台契约、runtime gate 与全部 state-machine 回归。
- `scripts/cargo-agent.sh check --lib --manifest-path src-tauri/Cargo.toml`
  - **PASS**。
- `scripts/cargo-agent.sh fmt --manifest-path src-tauri/Cargo.toml -- --check`
  - **PASS**。
- `git diff --check`
  - **PASS**。

未运行全量 Rust suite，未触发 quality-gate，符合 round 3 的聚焦验证约束。

### Round 3 residual risks / scope

- 本轮没有改变 production endpoint/public key fail-closed、metadata byte cap/redirect policy、opaque durable proof/lease、shared drain/post-stop coordinator、平台 installer 或 Tauri capability。
- Windows native NSIS 与 macOS 签名/公证 smoke 的既有 residual 不变；本轮仅修改 provider-neutral state/runtime 错误收敛，不声称新增原生平台覆盖。
- 本轮没有修改 Task 4 UI、Task 5/6 workflow、GitHub secrets/environments 或制品发布状态。

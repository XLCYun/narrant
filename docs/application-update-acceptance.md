# Application Update 验收记录

本记录对应 [`0017-application-updates.md`](./specs/0017-application-updates.md) 的 15 条验收标准。
它区分可在本机确定性执行的证据、必须由应用内浏览器采集的证据，以及只能在 GitHub 托管
runner 上产生的真实原生升级证据。除特别注明外，证据基线为源码仓库提交 `7c4a505` 及
Task 7 的 Preview Harness 路由修复。

## 验收结论边界

- 本机确定性测试：Task 7 已运行更新核心/adapter 的 84 个 Rust 测试、共享停止和 Babysit 的
  聚焦 Rust 测试，以及本文件“本轮实际执行”中列出的前端、脚本和静态门禁。
- 应用内浏览器：Task 4 的已提交报告
  [task-4-report.md](../.superpowers/sdd/2026-09-01-application-updates/task-4-report.md)
  记录了历史验收；父任务本轮又补充了 fresh 截图、可访问 DOM、实际 bounds、交互和 viewport
  恢复结果，见下方“Fresh 浏览器证据”。
- 托管原生 smoke：本机**没有**运行 macOS/Windows 原生安装升级，也不把脚本 fixture 当作原生
  执行。发布必须先通过 `.github/workflows/update-smoke.yml` 的 `native-smoke` 两目标 matrix 和
  `aggregate` job，再由 `.github/workflows/promote-stable.yml` 的 `promote` job 在受保护
  `stable-promotion` Environment 中复核 evidence 后发布。
- Rust 全量测试：父任务在 fresh 浏览器验收和 Preview 修复稳定后恰好运行一次。473 个 lib
  测试中 472 个通过；唯一失败是 2026-08-07 已存在的 Core Git 输出上限测试用瞬时
  `rev-parse` 模拟进程树终止，在全量并发下偶发先退出并令 macOS `killpg` 返回 `EPERM`。
  Task 7 将该测试夹具改为先输出越界字节、再保持存活，未修改生产进程实现；修后该测试
  50 次聚焦重复与相邻三个 Core Git process 测试均通过。依仓库“全量只运行一次”的节奏，
  没有再次运行全量。

## 15 条标准的证据矩阵

| # | 状态 | 可检查证据 |
|---|---|---|
| 1 | 本机通过 | `src-tauri/src/application_update/tests/state_machine.rs`：`ready_starts_identity_scoped_check_and_six_hour_schedule`、`startup_periodic_and_manual_checks_coalesce`、`readiness_controls_periodic_schedule_without_duplicate_check`、`semver_and_package_policy_fail_closed`、`automatic_download_preference_persists_and_resumes_candidate`。Task 7 的 `application_update` 聚焦集已运行这些测试。 |
| 2 | 本机通过 | `src-tauri/src/application_update/tests/preference.rs`：`missing_preference_uses_default_without_writing`、`preference_survives_repository_restart`；状态机测试 `automatic_download_preference_persists_and_resumes_candidate` 和 `disabling_auto_cancels_inflight_automatic_download` 证明关闭后只检查、重开后续传当前候选。 |
| 3 | 本机通过 | `progress_and_completion_are_identity_scoped`、`automatic_retry_uses_real_one_two_four_sequence`、`http_failures_map_to_stable_retry_categories`、`disabling_auto_invalidates_download_retry_but_explicit_retry_works`；`metadata.rs` 的 `status_timeout_malformed_and_missing_target_are_classified_exactly` 覆盖网络/HTTP/manifest 分类。 |
| 4 | 本机通过 | `src-tauri/src/application_update/tests/staging.rs`：`successful_higher_candidate_atomically_replaces_old_generation`、`every_pre_pointer_failure_recovers_old_generation`、`preinstall_reverification_rejects_package_and_metadata_tampering`、`completion_reconciliation_and_incomplete_recovery_cleanup`、`target_install_kind_and_stable_channel_are_enforced`；真实签名磁盘复验由 `verifier_checks_real_signature_against_disk_bytes_and_rejects_tampering` 覆盖。 |
| 5 | 聚焦通过；全量待父任务 | `src-tauri/src/tauri_app/tests/shutdown.rs`：`successful_stop_runs_steps_in_contract_order_and_is_installable`、`concurrent_and_repeated_callers_share_one_outcome`、`timeout_runs_the_final_exit_seam_but_never_becomes_installable`、`post_stop_executor_runs_real_outcome_install_and_final_action_matrix_once`、`exit_close_and_restart_compete_but_execute_one_installer_and_final_action`。本轮执行了第一项与 post-stop matrix；完整模块将在唯一 Rust 全量测试中再执行。 |
| 6 | 本机通过 | `src-tauri/src/operation/cancellable_tests.rs`：`close_and_drain_closes_admission_and_ignores_early_cancellations` 与 `close_and_drain_waits_for_every_running_registration_to_release`，本轮均已执行；`async_cancellation_wakes_managed_timer_and_drain_closes_future_admission` 还覆盖异步唤醒。 |
| 7 | 本机通过 | 生产收口点为 `BabysitApplication::shutdown`（`src-tauri/src/babysit/application.rs`），它遍历 registry capability 的同一 `shutdown` 契约；Codex、Claude 和 Custom/Built-in 分别由 `CodexCapabilityBundle::shutdown`、`ClaudeCapabilityBundle::shutdown`、`CustomCapabilityBundle::shutdown` 实现。测试 `shutdown_visits_every_registered_provider_once`、`shutdown_releases_the_owned_app_server_after_a_completed_run`、`application_delegates_installed_provider_lifecycle_through_one_runtime_seam`、`shutdown_clears_cache_and_notifies_the_provider_once` 本轮均通过。`src-tauri/src/application_update/` 不含任何 provider-specific 分支。 |
| 8 | 本机 adapter 通过；原生生命周期只在托管 runner | `src-tauri/src/application_update/tests/platform.rs`：`target_and_install_kind_must_both_match_the_selected_adapter`、`macos_original_path_replacement_rolls_back_when_candidate_move_fails`、`macos_real_filesystem_restores_old_app_after_candidate_or_post_failure`、`windows_current_user_nsis_args_only_relaunch_for_explicit_restart`、`windows_handoff_validates_x64_pe_copy_lifetime_argv_and_single_launch`；staging 清理由 `installed_result_survives_every_cleanup_failure_without_retrying_installer` 覆盖。上述 adapter 测试在本轮 84-test 集中通过；签名/公证 app 与真实 NSIS 的安装失败、权限和同路径替换由 `update-smoke.yml` 的两个 `native-smoke` job 执行，本机未运行。 |
| 9 | 前端测试通过 | `tests/application-update-components.test.tsx`：七状态 Settings table、四个 attention 状态、三个隐藏状态、Enter/Space、确定/不确定进度、安全 GFM、retry/restart intent 和双语文案；`tests/application-update-controller.test.tsx`：sequence 去旧、偏好回滚、重连、Retry/Restart 失败；`tests/tauri-application-update-adapter.test.ts` 证明只有一个 snapshot event 和五个窄命令。父任务 fresh 浏览器证据补足真实键盘/焦点。 |
| 10 | Fresh 浏览器通过 | `src/dev/preview.tsx` 的 `application-update` 组件声明 wide/short、narrow/long、determinate、indeterminate、failed、staged，Settings 还声明 `application-update` 场景。`tests/preview-routing.test.ts` 证明组件没有 `default` 时安全回退到首个已定义场景，修复了 fresh 验收发现的崩溃。父任务已采集本轮截图、DOM、bounds 与交互，并复验组件选择器回退、恢复 viewport 和关闭 tab，明细见下一节。 |
| 11 | 本机脚本测试通过 | `scripts/tests/release-assets.test.mjs`：`完整准备生成固定两目标 manifest并字面保留 canonical ED 签名文件`、`资产集合拒绝缺少、意外与重复名称`、`manifest 校验拒绝 URL、名称、版本、notes 和签名映射偏差`、`SHA256SUMS 只覆盖七文件且按公开名称稳定排序`；`scripts/tests/reconcile-draft-release.test.mjs` 覆盖 Draft 重建与 published fail-closed；`scripts/tests/release-workflow.test.mjs` 覆盖双平台签名构建与跨仓库身份。 |
| 12 | 本机脚本测试通过 | `scripts/tests/update-smoke-evidence.test.mjs`：`evidence set 拒绝重复、缺少、额外目标与跨 target Draft 漂移`、`freshness、future、checkpoint 和 source SHA 均 fail closed`、`normal evidence 绑定源码、Draft 八资产、previous package/signature 与真实 checkpoints`；`scripts/tests/promote-stable-release.test.mjs`：`evidence、Stable manifest 或 Draft asset 漂移时 PATCH 永不发生`；`scripts/tests/update-workflows.test.mjs` 固定 exactly-two matrix 和 protected Environment。 |
| 13 | 工作流门禁存在；本机未执行真实原生升级 | `scripts/tests/native-update-smoke-driver.test.mjs`：normal 与 bootstrap 的 checkpoint 顺序、durable staging 和 bootstrap recovery；`scripts/tests/promote-stable-release.test.mjs`：bootstrap exact tag、normal/bootstrap 隔离、历史 updater Stable 永久关闭 bootstrap。真实执行门禁是 `update-smoke.yml` 的 `native-smoke` matrix（`macos-15`/`darwin-aarch64` 与 `windows-2025`/`windows-x86_64`）及依赖它的 `aggregate`；promotion 必须下载该 run 的 attested exact-two evidence。Task 7 没有 dispatch 工作流，也没有远程运行记录。 |
| 14 | 文档通过 | [application-update-operations.md](./application-update-operations.md) 的“GitHub 配置”“正常发布流程”“一次性 first-updater bootstrap”“失败与恢复”“Updater 私钥备份、丢失与轮换”“每次发布检查表”分别覆盖密钥备份/丢失/泄露、old-key-signed A→B→C bridge、Draft 审批、失败恢复、bootstrap 和 immutable releases 未启用。 |
| 15 | 全部门禁已执行；全量中的既有测试夹具已聚焦稳定化 | 本轮运行 `pnpm test`、`pnpm test:scripts`、`pnpm exec tsc --noEmit`、`pnpm lint`、`pnpm build`、`scripts/cargo-agent.sh check --lib --manifest-path src-tauri/Cargo.toml`、聚焦 Rust 测试、fresh 应用内浏览器和 `git diff --check`。父任务恰好一次运行 `scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml`：472/473 通过，唯一失败是既有 Core Git 瞬时测试夹具；夹具稳定化后 50 次聚焦重复和相邻 3 个 process 测试通过。没有第二次全量运行，也没有手动触发 quality-gate。 |

## Fresh 浏览器证据（父任务执行）

截图位于本地忽略目录
`.superpowers/sdd/2026-09-01-application-updates/task-7-browser/`，未作为二进制提交：

- `wide-available.png`，1440 x 900：dialog 为 672 x 356，坐标 (384, 272)；attention icon
  为 32 x 32，坐标 (187, 845)；GitHub Identity 为 167 x 54，坐标 (12, 834)，二者间距
  8 px。body `scrollWidth/clientWidth` 为 1440/1440。可访问 DOM 包含 dialog 名称、版本、
  Release Notes、Later 和 Close。Enter 打开，Escape 后约 700 ms 将焦点恢复到
  “有应用更新可用”。
- `narrow-long-notes.png`，390 x 844：dialog 为 358 x 614.8，x=16；Release Notes 内层
  为 304 x 380，`scrollHeight=850`、初始 `scrollTop=0`；body 为 390/390。DOM 只有安全
  HTTPS 链接且没有 `img`。
- `downloading-determinate.png`，1000 x 800：dialog 为 672 x 416；progressbar 的
  `aria-valuenow=7340032`、`aria-valuemax=12582912`，label 为“更新已下载 58%”；body
  为 1000/1000。
- `downloading-indeterminate.png`：没有伪造 progressbar；status 明确总大小未知并显示已下载
  3.0 MB。
- `failed-retry.png`：初始有 alert、Later、Retry、Close；点击 Retry 后 dialog 保持打开，
  retry/error 消失并进入 checking。
- `staged-restart.png`：包含 Later、“立即重启并更新”和 Close，dialog 为 672 x 356，无溢出。
- `settings-wide.png`，1440 x 900：真实四 tab Settings shell，应用更新 tab 已选中；Switch
  初始开启且可关闭；手动检查进入“正在检查”并禁用按钮。
- `settings-narrow.png`，390 x 844：body 为 390/390，tablist 为 350/350；应用更新 tab
  已选中且完整控件可见。

Preview 路由的 fresh 复验也通过：从 `branch-selector/default` 用组件选择器切到 Application
Update 后，URL 可保留兼容性的 `scenario=default`，解析器会渲染第一个 `available` 场景；DOM 非空、
dialog 数量为 1，且没有 undefined-render 错误。父任务随后成功恢复默认 viewport、关闭 tab 并停止
Preview server。

## 托管原生 smoke 的精确发布门禁

本地脚本只证明 schema、路由、fail-closed 和 workflow 静态契约，不能产生 `real-native` evidence。
发布时必须满足以下顺序：

1. 手动运行 `.github/workflows/update-smoke.yml`；`native-smoke` 两目标均从固定 public Release ID
   API 回读 Draft，并在真实平台完成 bootstrap 或 previous-Stable-to-candidate 生命周期。
2. `aggregate` 只接受 `darwin-aarch64` 与 `windows-x86_64` 两份 fresh、checksummed、attested
   `real-native` evidence。
3. 手动运行 `.github/workflows/promote-stable.yml`；`promote` 下载指定 evidence run，验证 attestation，
   在 `stable-promotion` Environment 人工批准后再次回读 fixed-ID Draft，最后才执行 published/latest
   mutation。

Task 7 本机运行没有 GitHub Release、secret、Environment 或 workflow dispatch mutation。

## 本轮实际执行

以下结果由 Task 7 worker 与父任务在 2026-09-02 本机执行：

- `pnpm exec vitest run tests/preview-routing.test.ts`：3 passed。
- `scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml application_update -- --nocapture`：
  84 passed，389 filtered out。
- `scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml close_and_drain`：2 passed。
- `scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml successful_stop_runs_steps_in_contract_order_and_is_installable`：1 passed。
- `scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml post_stop_executor_runs_real_outcome_install_and_final_action_matrix_once`：1 passed。
- Babysit 四个 shutdown 聚焦测试：4 passed（命令和测试名见证据矩阵第 7 行）。
- `pnpm test`：37 files、307 tests passed。
- `pnpm test:scripts`：128 tests passed。
- `pnpm exec tsc --noEmit`：passed。
- `pnpm lint`：passed。
- `pnpm build`：passed，2032 modules transformed；Vite 报告既有大 chunk 提示。
- `scripts/cargo-agent.sh check --lib --manifest-path src-tauri/Cargo.toml`：passed。
- `scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml`：按 Issue 规则恰好运行一次；
  473 个 lib 测试中 472 passed、1 failed。失败测试为既有
  `core_git_process_classifies_the_stream_that_crosses_its_limit`，自动更新、共享停止和 Babysit
  相关测试在该运行中均通过。
- 将上述测试的瞬时 `rev-parse` 夹具改为确定性的“输出越界后保持存活”后，聚焦重复 50 次
  全部通过；`core_git::tests::core_git_process_` 三项聚焦测试随后 3/3 passed。
- `scripts/cargo-agent.sh fmt --check --manifest-path src-tauri/Cargo.toml`：passed。
- `git diff --check`：passed（Preview 修复后的聚焦执行；提交前将再执行）。
- `quality-gate`：未触发。

本机 Node 为 `v26.5.0`，高于仓库声明的 `<26` engine 上限；pnpm 给出 warning，但上述命令
全部成功。正式 CI 仍应使用仓库支持的 Node 版本。

诊断 Core Git 测试夹具时，额外的 20 次模块压力循环在第 14 次暴露了既有
`cancelled_workspace_creation_removes_its_partial_worktree` 清理竞态：取消恰好落在 Git 的
`locked initializing` 阶段时，测试可观察到临时 worktree 注册尚未被清理。它没有出现在本次
唯一全量运行，不由 Application Update 改动引入，也未在本需求中扩大为 Core Git 生产修复；
后续应由 Cancellable Core Git Operations 的 owning work 单独处理。

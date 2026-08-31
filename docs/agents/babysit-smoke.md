# Babysit smoke test

默认验证全部使用仓库内的确定性 fixture，不需要 provider 登录，也不会访问模型服务。
真实 endpoint probe 是明确 opt-in 的人工步骤，绝不属于 CI 或默认开发命令。

## CI 边界与数据安全

- CI 只运行 deterministic fixture、前端测试和 Preview Harness；默认测试命令不得调用本节的真实
  endpoint probe。`test:e2e:custom-live` 仅供 reviewer 已明确授权后的本机 opt-in 验证，禁止加入 CI。
- 只有 reviewer 明确同意一次本机 probe 后，才可以在 Application Settings 中填写其自己的
  OpenAI-compatible 或 Anthropic-compatible endpoint。不要在 shell 命令行、环境变量导出、
  issue、PR、日志、录屏或截图中放入 key 或完整 endpoint URL。
- probe 期间关闭 shell trace（例如 `set +x`），不要把输入重定向到文件；只在 UI 中输入值，
  并在结束后清空表单。不要复制或记录 prompt、source、response、Line Explanation 或仓库工具结果。
- 失败时只记录稳定的错误类别和是否完成，不记录 provider 原文、请求/响应 body、完整路径或
  诊断以外的上下文。不要点击复制诊断，除非确认其内容仍只包含稳定类别。

## 默认 deterministic fixture 流程

1. 在 Git Bash、macOS 或 Linux shell 中运行下列 fixture；Windows PowerShell 应显式使用
   `C:\Program Files\Git\bin\sh.exe` 代替 `sh`，避免系统询问如何打开 `.sh`：

   ```sh
   sh scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml babysit::tests::provider_registry
   sh scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml babysit::tests::fake_provider_counts_invalid_generate_and_repair_rounds -- --exact
   sh scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml babysit::tests::claude::cancellation_discards_output_and_terminates_the_process -- --exact
   ```

   这些测试覆盖 provider 选择与冻结快照、生成/修复、取消和清理，全部使用仓库 fixture。
2. 运行 `pnpm test -- --run` 与 `pnpm dev:preview`，不设置 provider 凭据。
3. 在 Application Settings 的宝宝模式分区确认首次设置默认选中 Custom，并显示未配置编辑器。
4. 在 Custom 编辑器确认 protocol、Base URL、可选 API key、model 和 effort 可以作为一个完整连接
   保存；确认 key 默认隐藏，且只在按钮触发后显示。
5. 在 Review Reader Preview 验证 generating、repairing、failed、Custom 未配置恢复和取消按钮。
   恢复动作必须打开 Application Settings，不得建议回退到 Codex 或 Claude Code。
6. 使用明确的宽、窄 viewport 检查长值截断、下拉展开、saving/failed 状态、sticky footer、关键
   按钮尺寸和没有水平溢出。

## Opt-in 真实 endpoint probe（人工、非 CI）

执行前先取得 reviewer 对可能产生费用和发送代码内容的明确同意，并使用无敏感信息的 disposable
Review Workspace。整个 probe 只观察状态，不保存或分享任何模型输入输出。

本地 Debug 已经通过设置页保存连接时，可运行 `pnpm test:e2e:custom-live`。该 wrapper 直接读取应用
数据目录中的 `settings.json` 与 `custom-provider-credential.dev.json`，把值仅传给本次子进程，并用
隔离的 E2E settings/database 控制真实 Tauri UI；它不会打印 key 或完整 endpoint。用例必须完成
设置保存、打开 disposable fixture、启动宝宝模式，并确认 Diff 中出现非空逐行解释。文件名使用
`.smoke.ts`，因此默认 `test:e2e` 的 `*.e2e.ts` glob 不会运行它。

该命令是本地当前 OpenAI Chat Completions 配置的窄回归，只覆盖一次生成与真实渲染；不替代下面的
完整人工发布 smoke。无鉴权 endpoint 可以没有 `custom-provider-credential.dev.json`。WDIO 结束时会
删除本次隔离的 settings、database 与 fixture，不修改本地正式连接。

## 完整人工发布 smoke

对每个协议各执行一次以下流程：

1. 打开 Application Settings → 宝宝模式 → Custom，明确选择目标协议。将 reviewer 提供的
   Base URL、可选 key、model 和 effort 直接输入表单并保存；不要把这些值放进命令行、终端历史、
   自动化脚本或截图。
2. 打开一个受信任的 disposable Review，选择一个代码 File Change，启动 Babysit。只确认请求
   经过 generating（需要时可观察 repairing）并最终显示 completed，或只记录稳定失败类别；
   不复制、粘贴、导出或截图 source、prompt、response、Line Explanation 或 repository tool result。
3. 关闭 Babysit 后再打开同一个 File Change，确认完成结果可以复用；点击重新生成和取消各一次，
   只观察状态变化，不记录返回内容。
4. 清空或替换 Custom 连接中的 reviewer 值，关闭 disposable Review，并删除任何临时本地数据。

该流程对 OpenAI Chat Completions 与 Anthropic Messages 分别执行，协议必须由 reviewer 明确选择，
不得根据 URL 推断。probe 的唯一可提交记录是日期、协议、成功/失败类别和客户端版本；严禁提交
key、完整 endpoint URL、prompt、source、response、解释、工具结果或 provider 原始诊断。

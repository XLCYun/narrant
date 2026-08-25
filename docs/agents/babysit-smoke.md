# Babysit 本地 smoke test

默认 smoke 使用仓库内的确定性 provider fixtures，不需要安装、登录 Codex 或 Claude Code，
也不会产生付费模型请求。只有明确的发布前本机 probe 才能使用真实 provider。

## 默认 fixture 流程

1. 在 Git Bash、macOS 或 Linux shell 中运行下列 deterministic backend fixtures；Windows PowerShell
   应显式使用 `C:\Program Files\Git\bin\sh.exe` 代替 `sh`，避免系统询问如何打开 `.sh`：

   ```sh
   sh scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml babysit::tests::provider_registry
   sh scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml babysit::tests::fake_provider_counts_invalid_generate_and_repair_rounds -- --exact
   sh scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml babysit::tests::claude::cancellation_discards_output_and_terminates_the_process -- --exact
   ```

   这些测试分别验证 selected-provider 路由与冻结快照、生成/修复，以及取消和子进程清理；
   全部使用仓库 fixture，不读取真实凭据、不访问模型服务。
2. 运行 `pnpm test -- --run` 与 `pnpm dev:preview`，不设置任何 provider 凭据。
3. 在 Application Settings 的宝宝模式分区确认新设置默认选中 Codex。
4. 切换 Codex 与 Claude Code，分别修改 executable、model 和 effort；往返切换后确认两套偏好独立保留。
5. 验证 Codex 模型目录的 loading、empty、error/重试与 model 变化后不兼容 effort 原子回到 Default。
6. 在 Review Reader preview 验证 generating、repairing、failed 和 provider setup failure。setup failure 的“打开应用设置”必须直达宝宝模式分区，不得显示 Diff footer executable picker。
7. 用明确的宽、窄 viewport 检查长路径、下拉展开、sticky footer 和关键按钮的实际尺寸。

## 可选真实 provider probe

只在维护者明确同意使用本机已登录 provider 及可能的付费请求时执行。在 Application
Settings 中选择目标 provider 和 executable，打开受信任 Review 的代码 File Change，再验证生成、
修复、缓存复用、重新生成、取消和线程清理。不得把真实源码、完整路径、prompt、token、
provider 原始输出或 stderr 写入 issue、日志和诊断文本。

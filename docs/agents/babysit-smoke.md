# Babysit 本地 smoke test

默认 smoke 使用仓库内的确定性 provider fixtures，不需要安装、登录 Codex 或 Claude Code，
也不会产生付费模型请求。只有明确的发布前本机 probe 才能使用真实 provider。

## 默认 fixture 流程

1. 运行前端测试与 `pnpm dev:preview`，不设置任何 provider 凭据。
2. 在 Application Settings 的宝宝模式分区确认新设置默认选中 Codex。
3. 切换 Codex 与 Claude Code，分别修改 executable、model 和 effort；往返切换后确认两套偏好独立保留。
4. 验证 Codex 模型目录的 loading、empty、error/重试与 model 变化后不兼容 effort 原子回到 Default。
5. 在 Review Reader preview 验证 generating、repairing、failed 和 provider setup failure。setup failure 只能打开 Application Settings，不得显示 Diff footer executable picker。
6. 用明确的宽、窄 viewport 检查长路径、下拉展开、sticky footer 和关键按钮的实际尺寸。

## 可选真实 provider probe

只在维护者明确同意使用本机已登录 provider 及可能的付费请求时执行。在 Application
Settings 中选择目标 provider 和 executable，打开受信任 Review 的代码 File Change，再验证生成、
修复、缓存复用、重新生成、取消和线程清理。不得把真实源码、完整路径、prompt、token、
provider 原始输出或 stderr 写入 issue、日志和诊断文本。

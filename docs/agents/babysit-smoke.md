# Babysit 本地 smoke test

CI 使用 fake JSONL App Server，不需要 Codex 登录或网络。发布前可在本机用已安装的
Codex 做一次真实 smoke；该流程当前固定使用 `gpt-5.6-luna` 与 `low` reasoning，且不会把
代码解释写回 Review Workspace。

1. 确认 `codex --version` 可执行，并已通过 Codex 支持的任一账户类型完成认证。
2. 启动 Narrant，打开一个已授予 Workspace Trust 的 Review。
3. 进入一个 `.ts`、`.tsx`、`.js`、`.jsx`、`.rs`、`.py`、`.go`、`.java`、`.c` 或
   `.cpp` File Change，点击宝宝模式。
4. 确认首次点击后才出现 Codex discovery/初始化，Diff 在生成期间仍可阅读，最终
   before/after 解释都显示在对应行上方。
5. 切换“改变行/全部非空行”、隐藏后重新打开宝宝模式，并切换到另一个 File Change；
   确认隐藏/返回已完成文件不产生新的模型 turn，而切换文件会取消旧 turn。
6. 在生成期间刷新 Snapshot、关闭 Review，再退出应用；确认旧解释不再出现在新
   Snapshot，Codex history 中没有 Narrant 的 Babysit thread。

如果 Codex 未安装、未登录或 capability probe 失败，记录 UI 中的 typed recovery
状态即可；不要把真实源码、完整路径、prompt、token 或 App Server 原始输出复制到
issue、日志或诊断文本中。
# Claude Code 默认 provider 本地 smoke

1. 确认已安装并登录 Claude Code，然后启动 Narrant，打开一个受信任 Review 的文本 File Change。
2. 打开宝宝模式，确认此时才首次启动 Claude Code discovery/probe；应用启动阶段不应出现 Claude 进程。
3. 验证生成中、修复中和完成状态；关闭再打开应复用进程内缓存，“重新生成”必须发起 fresh Claude Code rounds。
4. 临时选择无效 executable，确认只显示 Claude Code 选择器恢复动作，且不会启动或建议 Codex。
5. 在未登录与旧版 Claude Code 环境分别确认登录、升级恢复文案；诊断不得包含 executable 路径、仓库内容或原始 stderr。
6. 用 `pnpm dev:preview` 检查 `review-reader` 的 `babysit`、`babysit-generating`、`babysit-repairing`、`babysit-claude-recovery`、`babysit-claude-signed-out`，并分别使用宽、窄 viewport。

# Babysit 本地 smoke test

CI 使用 fake JSONL App Server，不需要 Codex 登录或网络。发布前可在本机用已安装的
Codex 做一次真实 smoke；该流程不指定模型，也不会把代码解释写回 Review Workspace。

1. 确认 `codex --version` 可执行，并在 Codex CLI 中完成登录。
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

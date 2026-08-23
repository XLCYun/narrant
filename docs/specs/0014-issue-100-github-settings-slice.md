# GitHub Identity Application Settings 分片

## Problem Statement

Application Settings 的 GitHub 分区目前只是占位内容。已登录用户无法在设置页确认当前 GitHub Identity，也无法在了解本机数据影响后安全退出；现有侧栏快捷退出必须保持原有低摩擦行为。

## Solution

让设置页消费现有 GitHub Identity lifecycle 的窄绑定，在 GitHub 分区展示当前身份，并提供解释性确认退出。确认后直接调用现有 lifecycle；退出失败时保留身份、展示错误并允许重试。侧栏入口继续复用同一个 lifecycle，但不增加确认步骤。

## User Stories

1. 作为已登录用户，我希望在 GitHub 设置分区看到当前名称、login 与头像，以便确认当前授权身份。
2. 作为只存在缓存身份的用户，我希望仍能看到该身份，以便 GitHub 暂时不可用时不丢失上下文。
3. 作为准备退出的用户，我希望先看到确认说明，以便知道本机授权凭据与身份/头像缓存会被删除。
4. 作为准备退出的用户，我希望确认说明明确 Repository、Review 与本地代码不会删除，以便放心操作。
5. 作为取消退出的用户，我希望身份和授权状态完全不变。
6. 作为确认退出的用户，我希望设置页复用现有 GitHub Identity lifecycle，以便所有入口具有一致结果。
7. 作为遇到退出失败的用户，我希望当前身份仍显示、错误可见且可以重试，以免界面虚假显示已退出。
8. 作为高频用户，我希望侧栏快捷退出保持无需确认，以便原有工作流不增加步骤。

## Implementation Decisions

- GitHub 分区接收现有 GitHub Identity lifecycle 的公开绑定，不创建新的退出状态、adapter 或持久化边界。
- 已登录状态展示与全局入口相同的公开身份语义：头像、显示名称与 `@login`；公开身份缺失时仍展示已连接状态。
- 设置页退出按钮打开 shadcn Dialog。确认文案明确删除本机 GitHub 授权凭据及身份/头像缓存，同时明确不删除 Repository、Review 或本地代码。
- 只有确认操作调用 lifecycle `signOut`。取消及关闭弹层不产生退出调用。
- 退出进行中禁用确认操作并显示进行中状态，避免重复提交。
- lifecycle 报告退出失败时保留已确认的 GitHub Identity，GitHub 分区展示错误和重试操作；重试再次调用同一 `signOut`。
- App 组合根把同一个 lifecycle binding 传给设置页和侧栏入口。侧栏菜单保持现有直接调用 `signOut` 的行为。
- 新增或修改的方法、结构体和组件提供易读中文注释。

## Testing Decisions

- 已确认测试缝为应用壳 UI 与现有 GitHub Identity lifecycle。
- 组件测试通过用户可见角色与文案验证身份展示、确认/取消、成功退出、失败保留身份与重试，不测试 Dialog 或 hook 的私有实现。
- 既有 GitHub Identity lifecycle 测试作为失败保留身份的先例；既有侧栏 GitHub Identity Entry 测试必须继续证明快捷退出无需确认。
- 使用 Preview Harness 渲染已登录、确认弹层与失败状态，并在宽、窄 viewport 做真实浏览器检查。

## Out of Scope

- GitHub 登录与 Device Flow 重设计。
- 改变侧栏快捷退出的确认语义。
- 将凭据或身份缓存写入 `settings.json`。
- GitHub App 安装、权限或 Pull Request Discovery 行为修改。
- 外观与宝宝模式设置实现。

## Further Notes

- 受 `docs/specs/0014-application-settings.md` 与 `docs/specs/0005-global-github-identity-entry.md` 约束。
- 本分片仅实现 Issue #100，不发布或修改 GitHub Issue。

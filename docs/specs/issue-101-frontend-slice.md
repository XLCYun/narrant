## Problem Statement

用户缺少在 Application Settings 中发现、选择与清空 Codex Executable Preference 的界面，Review Reader 仍暴露第二套临时选择入口。

## Solution

在宝宝模式设置分区提供当前偏好、原生文件选择、保存中与错误状态及“恢复自动发现”操作；通过窄 typed command adapter 调用后端，并移除 Review Reader 的临时选择入口。

## User Stories

1. 作为桌面用户，我希望通过系统文件选择器选择 Codex，以避免手输路径。
2. 作为已配置用户，我希望看到当前路径并能清空，以恢复自动发现。
3. 作为验证失败用户，我希望原设置保留并看到可操作错误。
4. 作为 Review Reader 用户，我希望错误引导我进入 Application Settings，而不是提供另一套选择器。
5. 作为窄窗口用户，我希望长路径与操作仍可使用。

## Implementation Decisions

- 设置页仅通过原生文件选择器获得候选路径，不提供文本输入。
- Tauri typed command contract 暴露选择、验证并保存，以及清空偏好的窄操作。
- UI 平时展示“自动发现”或已保存路径；验证期间禁用重复提交；失败保留旧投影。
- Review Reader 移除选择按钮、adapter 能力和生命周期回调，错误文案指向 Application Settings。
- 新增或修改的方法、结构体和组件使用易读中文注释。

## Testing Decisions

- 设置 UI 公共组件测试覆盖选择成功、取消、验证失败、清空与长路径展示。
- Review Reader 公共呈现测试证明临时选择操作消失。
- 使用通用 Preview Harness 检查宽窄窗口、长路径、加载与错误状态；若浏览器因 trusted-path 阻塞，记录一次即可。

## Out of Scope

- 自由文本路径输入。
- 模型与思考强度设置。
- GitHub Identity 与主题实现。

## Further Notes

- 后端验证和权威解析由后端分片提供，UI 不复制校验逻辑。

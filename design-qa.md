# PR 状态筛选与行内标签视觉验收

final result: passed

## 本轮：筛选栏中文文案

- 补齐中文筛选词条：“已开启 / 已关闭 / 已合并 / 全部”，前三项与行内状态胶囊一致。英文保留“Open / Closed / Merged / All”。
- 中文空结果提示改为“没有{{state}}的 Pull Request”，例如“没有已开启的 Pull Request”。
- 应用内浏览器实际 900×720、390×844 检查通过；保留用户原有缩放，读取 innerWidth / innerHeight 核实 CSS 尺寸。窄窗口四个中文标签完整显示，筛选栏宽约 312–319px，页面无横向溢出。
- “已开启 / 已关闭 / 已合并 / 全部”分别显示 2 / 1 / 1 / 4 条记录；浅色与深色空结果提示均使用中文，从空结果切换“已关闭”可恢复条目。长标题仍截断，草稿、状态胶囊与审核按钮完整可见。
- 相关组件与 App 的 50 项现有测试通过，包括中文四态筛选、英文四个标签和空结果、切换仓库恢复默认状态；TypeScript 检查与 Lint 通过。
- 浏览器控制台无 error/warn，已恢复默认 viewport。
- 宽窗口截图：`/Users/lizhanfa/.codex/visualizations/2026/10/18/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-localized-filters-light.png`。
- 窄窗口和深色空结果截图：同目录 `issue-202-localized-filters-narrow.png`、`issue-202-localized-filters-empty-dark.png`。

## 前轮：行内状态胶囊标签

- 用户标注的是 PR 行内作者旁边的 `open` 文本，要求更醒目的胶囊标签与不同状态颜色。
- 复用现有 shadcn Badge 的尺寸与圆角；Open 使用绿色、Closed 使用红色、Merged 使用紫色，搭配状态图标、淡底色和细边框。筛选与行标签共用生命周期图标定义，筛选按钮继续使用默认 Tabs 样式。
- 复用现有 `review.state` 中英文文案：中文显示“已开启 / 已关闭 / 已合并”，英文显示“Open / Closed / Merged”。草稿仍单独显示，未知状态保留原文并使用中性标签。
- 应用内浏览器在实际 1080×840 和 390×844 下检查了浅色、深色、长短标题及三个生命周期状态。中文状态标签均为 68×24px，使用 Badge 默认胶囊圆角；图标隐藏于可访问名称之外，状态文字完整可读。
- 390px 窗口中四条记录的状态标签与审核按钮完整显示；长标题截断，分支关系换行；页面没有横向溢出。Open / Closed / Merged / All 仍分别显示 2 / 1 / 1 / 4 条记录，上方任务标签保持 Pull Requests。
- 浏览器控制台无 error/warn；结束时恢复默认 viewport（1280×720），保留浅色 All 预览以便对比三种颜色。
- 本轮相关 23 项组件测试、TypeScript 检查、Lint 通过；样式调整未新增重复测试。
- 浅色截图：`/Users/lizhanfa/.codex/visualizations/2026/10/18/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-status-badges-light.png`。
- 深色截图：`/Users/lizhanfa/.codex/visualizations/2026/10/18/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-status-badges-dark.png`。
- 窄窗口截图：同目录 `issue-202-status-badges-narrow.png`、`issue-202-status-badges-narrow-dark.png`。

## 前轮：筛选按钮样式对照依据

- 用户选定图：`/Users/lizhanfa/.codex/generated_images/01a14ae8-0cdc-7b31-9b86-dbc4750f8e97/exec-cba8ca4d-b334-457c-9b1c-60e9f2a3b6ed.png`。
- 最新浏览器批注要求：状态按钮组与上方 Pull Requests / 本地分支 / 审核历史使用相同样式。
- 两组按钮都直接使用现有 shadcn Tabs、TabsList、TabsTrigger。移除状态按钮单独添加的颜色和圆角覆盖，保持图标与筛选行为。
- 浅色截图：`/Users/lizhanfa/.codex/visualizations/2026/10/18/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-default-tabs-light.png`。
- 深色截图：`/Users/lizhanfa/.codex/visualizations/2026/10/18/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-default-tabs-dark.png`。
- 窄窗口截图：`/Users/lizhanfa/.codex/visualizations/2026/10/18/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-default-tabs-narrow.png`。
- 截图上方是通用开发预览工具栏，不属于产品 UI。生成图中的绿色选中态已由用户最新批注明确改为共享组件的默认样式。

## 前轮筛选按钮检查

- 应用内浏览器实际渲染尺寸为 900×600 和 390×844。浏览器保留用户原有缩放，通过 viewport 设置并读取 innerWidth / innerHeight 确认实际 CSS 尺寸。
- 浅色下，上方 Pull Requests 与下方 Open 的计算样式完全一致：白色背景、前景色文字、透明边框、14px 圆角；按钮高度均约 29px。
- 深色下，两组选中按钮的背景、文字、边框和圆角也完全一致，直接来自 TabsTrigger 默认主题样式。
- 未选中的状态按钮与上方未选中任务标签使用相同的透明背景和次级文字颜色。
- 390px 下，状态按钮组宽约 309.5px，四个状态完整显示；document.scrollWidth 为 390px，没有横向溢出。长标题仍截断，草稿与审核操作可见。
- Open / Closed / Merged / All 分别显示 2 / 1 / 1 / 4 条代表性记录；鼠标、方向键、Enter、Home 和 Space 操作可用，上方 Pull Requests 始终保持选中。
- 图标继续使用 Hugeicons：Open=GitPullRequestIcon，Closed=GitPullRequestClosedIcon，Merged=GitMergeIcon，All=ListViewIcon。
- 浏览器控制台无 error/warn；结束时恢复浅色、Open 和默认 viewport（857×1289）。

## 前轮验证范围

- 本轮 `tests/repository-overview.test.tsx` 的 23 项测试通过，覆盖四态筛选、在线元数据优先、空结果和刷新保留等行为；未为样式删除新增重复测试。
- 此前本 PR 的 50 项相关组件和 App 测试、1440px 宽窗口、空结果恢复、刷新、任务标签往返、ARIA 关联及审核动作检查已通过。
- 本轮没有修改共享 Tabs、筛选逻辑、业务数据或 Tauri 代码。使用通用 Preview Harness 验收，完整 CI 结果以最新 PR 提交为准。

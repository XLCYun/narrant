# PR 状态分段筛选视觉验收

final result: passed

## 对照依据

- 用户选定图：`/Users/lizhanfa/.codex/generated_images/01a14ae8-0cdc-7b31-9b86-dbc4750f8e97/exec-cba8ca4d-b334-457c-9b1c-60e9f2a3b6ed.png`。
- 用户补充：四个可切换状态按钮使用正确图标与圆角，与上方任务导航复用同一个组件。
- 实现截图：`/Users/lizhanfa/.codex/visualizations/2026/10/17/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-segmented-900.png`。
- 全图对照：`/Users/lizhanfa/.codex/visualizations/2026/10/17/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-segmented-comparison.png`。
- 工具栏局部对照：`/Users/lizhanfa/.codex/visualizations/2026/10/17/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-segmented-toolbar-comparison.png`。
- 参考图像素为 1536×1024，按比例归一为 900×600。实现视口与截图均为 900×600（每 CSS px 对应 1 图像 px）。实现上方约 80px 为通用开发预览工具栏，不属于产品 UI；工具栏局部已按各自实际位置裁切，比较相同的 Open 选择、浅色主题和两条代表性 PR。
- 本次是在现有应用内替换筛选入口，保留仓库字体、字号及列表行结构；按钮密度和圆角按用户要求沿用现有 shadcn Tabs，不复制生成图中的宽矩形分段。

## 发现与修复

1. 首轮深色检查发现状态文字直接使用品牌主色时偏暗，属于 P2。
   - 修复前：`/Users/lizhanfa/.codex/visualizations/2026/10/17/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-segmented-dark-before.png`。
   - 修复：沿用现有 PR 状态徽章的浅翠绿色文字，配合淡翠绿色背景及边框；浅色主题保持品牌绿色。
   - 修复后：`/Users/lizhanfa/.codex/visualizations/2026/10/17/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-segmented-dark.png`。文字从 oklch(0.448 0.119 151.328) 改为 oklch(0.845 0.143 164.978)，视觉复查清楚可辨。
2. 复查无尚需处理的 P0/P1/P2 问题。

## 五项视觉检查

- 字体：沿用 Oxanium；标签、标题、元信息层级清楚，长标题截断后草稿和审核操作仍可见。保留既有 14px 标签字号是产品约束。
- 布局：状态控件与列表位于同一边框；两组均使用 Tabs、TabsList、TabsTrigger。390×844 下工具栏自然换行，四个状态全部显示；页面宽度为 390px，筛选组实测约 311px，无横向溢出。1440×900 下列表仍维持 max-w-6xl。
- 颜色：浅色绿色选中态、上方白色任务标签与参考图一致；深色修复后选中态清晰，焦点与选中状态独立。
- 图标与素材：全部使用项目现有 Hugeicons。Open=GitPullRequestIcon，Closed=GitPullRequestClosedIcon，Merged=GitMergeIcon，All=ListViewIcon；没有新增位图或自绘图标。
- 文案：保留四态英文名称、状态标签、草稿、开始/继续审核和原有空结果说明；编号、作者及时间沿用真实组件规则。预览相对时间随时钟变化，未写死为参考图的“9小时前”。

## 交互与证据

- 宽窗口 1440×900、常规窗口 900×600、窄窗口 390×844；浅色与深色均实际渲染。
- 依次切换 Open / Closed / Merged / All，显示 2 / 1 / 1 / 4 条代表性记录。
- 方向键移动到 Merged 时 Closed 仍选中；Enter 选择 Merged，Space 可选择 Open。上方任务导航独立。
- 已核对选中 Tab 的 aria-controls 指向实际面板，面板 aria-labelledby 指回选中 Tab。
- 切去本地分支再返回保留 All；空筛选切换 All 恢复条目；重试刷新后仍选中 Merged，新增合并状态条目进入列表。
- 继续审核触发正确预览动作；控制台无 error/warn。
- 窄屏：`/Users/lizhanfa/.codex/visualizations/2026/10/17/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-segmented-390.png`；空结果：`/Users/lizhanfa/.codex/visualizations/2026/10/17/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-segmented-empty.png`；刷新：`/Users/lizhanfa/.codex/visualizations/2026/10/17/01a14ad4-b2f1-7180-bde1-50473bfcfa3a/issue-202-segmented-refresh.png`。
- 已恢复默认 viewport。无新的弹层，原分支按钮等交互未变。

## 实现检查清单

- [x] 与任务导航复用 shadcn Tabs，保留独立选择上下文。
- [x] 使用正确图标、圆角和清晰选中态。
- [x] 四态筛选、空结果、导航往返、刷新及审核动作可用。
- [x] 宽窄布局、长短内容、明暗主题和键盘操作通过浏览器检查。
- [x] 50 项相关组件和 App 测试通过；静态双路审查无可操作发现。

验收范围为真实 React 组件的通用 Preview Harness；未修改业务数据，未将其表述为原生桌面 E2E 或完整 CI 通过。

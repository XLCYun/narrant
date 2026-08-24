# Application Settings Integration Acceptance Slice

## Problem Statement

Application Settings 的四个交付分片已经实现，但仍需在同一应用壳和 Babysit Agent Provider 链路中完成跨分区验收。当前缺少统一证据证明设置导航、主题、GitHub Identity、Codex Executable Preference 与 Babysit Model Preference 在宽窄窗口、失败状态、生产构建和完整 Rust 契约下可以共同工作。

## Solution

以应用壳 UI 和公开 Rust/Tauri 契约为两个最高测试缝，整合并验证全部设置功能。补齐跨分区行为与可访问性缺口，使用通用 Preview Harness 真实渲染代表性状态，并执行 TypeScript、生产构建、Rust 全量测试及代码审查门禁。

## User Stories

1. 作为桌面用户，我希望从任意主要目的地进入设置并返回原页面，以便不丢失工作上下文。
2. 作为键盘或辅助技术用户，我希望设置入口、二级导航、表单和确认弹层都有可靠名称、焦点与状态，以便完整操作。
3. 作为窄窗口用户，我希望外观、GitHub 和 Babysit Mode 设置仍可阅读和操作，以便无需扩大窗口。
4. 作为主题用户，我希望主题变化在应用壳、编辑器与品牌图像中一致，以免出现混合主题。
5. 作为 GitHub 用户，我希望设置页退出确认与侧栏快捷退出共享身份生命周期，同时保持不同确认语义。
6. 作为 Babysit Mode 用户，我希望长 Codex 路径、验证错误、清空和自动发现状态在设置页中清楚呈现。
7. 作为模型用户，我希望普通查看不连接 App Server，而修改时能看到加载、空目录、错误、重试、保存和取消状态。
8. 作为显式配置用户，我希望无效 Codex 路径或模型能力产生可操作错误且不静默回退。
9. 作为维护者，我希望前后端 typed command、设置持久化与 Babysit Agent Provider 解析保持一致，以免跨分片契约漂移。
10. 作为发布负责人，我希望自动化、真实渲染与生产构建共同通过，以便设置功能达到可交付状态。

## Implementation Decisions

- 前端分片负责应用壳全量交互、响应式布局、可访问性、Preview Harness 场景、真实浏览器证据、TypeScript 门禁与生产构建。
- Rust/契约分片负责 Application Settings、Tauri typed command、Codex Executable Preference 和 Installed Agent Provider 的跨域契约，以及聚焦 Rust 验证准备。
- 两个分片只修复公开行为的集成缺口，不扩大 0014 规格范围，也不新增设置分区。
- 视觉验收必须使用 `pnpm dev:preview` 与应用内浏览器尝试一次；若 trusted RPC path 阻塞，记录精确错误，不改用替代浏览器工具。
- 最终集成分支统一执行 TypeScript 测试、类型检查、Lint、生产构建、`git diff --check`，以及通过仓库锁脚本执行一次 Rust 全量测试。
- Windows 测试 harness 的既有失败必须用基线对照区分，不把基线缺陷误报为本次回归。
- 新增或修改的方法、结构体、类和组件提供易读中文注释；Rust 私有测试继续放在模块 `tests/` 目录。

## Testing Decisions

- 前端只在应用壳 UI 公共 seam 验证：设置进入/返回、二级导航、即时主题与回滚、GitHub 退出、Codex 路径、模型编辑和错误状态。
- Rust 只在 Application Settings、Babysit Agent Provider 与 typed Tauri command 公共 seam 验证，不直接测试私有 JSON、进程或协议 helper。
- 工蜂使用 red-green 垂直切片，只运行各自受影响的聚焦测试；最终全量门禁由雄蜂合并后执行一次。
- Preview Harness 覆盖宽/窄 viewport、长短内容、GitHub 确认弹层、模型加载/空/错误、Codex 验证错误及三态主题，并在结束后恢复默认 viewport。
- 审查按 Standards 与 Spec 两轴并行进行；发现问题后修复并重跑受影响门禁，直到两轴通过。

## Out of Scope

- 新增设置类别、重做 GitHub OAuth、自由输入路径或模型、持久化模型目录、修改运行中的分析，以及发布或关闭 GitHub Issue。
- 使用应用内浏览器以外的替代工具伪造视觉验收结果。

## Further Notes

- 本分片受 `docs/specs/0014-application-settings.md`、ADR-0022、ADR-0023 及 Issue #99–#102 的全部 slice spec 约束。
- 若 Git 写权限失败，工蜂保留工作区并向雄蜂报告 ready，由上级代为提交。

## Verification Record

- 应用壳 UI 聚焦测试 `tests/settings-shell.test.tsx`：23 项通过；typed command/adapter 聚焦测试：26 项通过。
- TypeScript typecheck、Lint、生产构建与 `git diff --check` 通过。
- 权限恢复后的 TypeScript 全量测试共 241 项：239 项通过；GitHub Identity 超时项串行复跑 6/6 通过；未被本分支修改的 `file-type-icons-build` 在当前机器持续超过硬编码的 5 秒测试预算，仍是独立的基线门禁风险，不能宣称全量通过。
- `pnpm dev:preview` 与应用内 Browser 在权限恢复后成功连接。1440×900 的长 Codex 路径场景和 390×844 的窄窗口均无横向溢出；GitHub 退出确认弹层尺寸与可访问 DOM 正常；验收后已恢复默认 viewport。
- `scripts/cargo-agent.sh check --lib` 通过。最终 Rust 全量测试执行一次，在 Windows 基线 harness 编译阶段被 8 个既有错误阻断：Unix-only `PermissionsExt` fixtures 以及 `core_git` 私有 helper 可见性；错误与本集成分片无代码差异，未重复运行。
- Standards 审查发现的领域术语漂移已修复；Spec 审查发现的门禁证据缺失通过本记录修复。

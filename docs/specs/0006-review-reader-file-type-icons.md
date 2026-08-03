# Review Reader File Type Icons

## Problem Statement

Review Reader 当前仅用文字展示目录和文件路径，并用 Hugeicons 表达 File Change 状态。面对包含大量 File Changes 的 Review Snapshot，阅读者必须逐行阅读名称才能辨认文件与目录类型，难以像在熟悉的开发工具中一样快速扫读文件树。目录展开状态、文件类型和 File Change 状态也缺少彼此独立的视觉层级。

## Solution

在 Review Reader 文件树中加入 Material Icon Theme 风格的彩色 **File Type Icon**：目录根据名称和展开状态显示对应图标，文件根据新路径的精确文件名或扩展名显示对应图标，未知类型使用通用图标。选中 File Change 的 Diff 标题复用相同图标。File Type Icon 只表达类型，现有 Hugeicons 继续独立表达新增、删除、修改和重命名状态。

所有图标随应用离线打包，并自动适配明暗主题。生产构建从固定主题配置计算 resolver 可能返回的完整图标闭包，校验资产零缺失，报告图标数量和原始总大小；超过 2 MiB 时警告但不裁剪资产、不阻断构建。

## User Stories

1. 作为 Review 阅读者，我希望在文件名左侧看到熟悉的文件类型图标，以便快速扫读大量 File Changes。
2. 作为 Review 阅读者，我希望 TypeScript、Rust、Markdown、JSON 等常见类型具有可区分的彩色图标，以便减少逐字阅读路径的负担。
3. 作为 Review 阅读者，我希望 `Dockerfile`、`.gitignore`、`package.json` 等特殊文件名得到准确图标，以便无扩展名或特殊命名文件不会被误判。
4. 作为 Review 阅读者，我希望复合扩展名按最长、最具体的规则匹配，以便 `*.test.ts` 等名称得到稳定结果。
5. 作为 Review 阅读者，我希望未知文件类型显示通用文件图标，以便任何 File Change 都有一致的视觉占位。
6. 作为 Review 阅读者，我希望目录行显示文件夹图标，以便目录与文件一眼可分。
7. 作为 Review 阅读者，我希望 `src`、`tests`、`docs`、`.github` 等已知目录显示专属图标，以便更快识别常见代码库结构。
8. 作为 Review 阅读者，我希望未知目录显示通用文件夹图标，以便任意仓库结构都能正常呈现。
9. 作为 Review 阅读者，我希望目录图标随展开和折叠状态变化，以便当前层级状态清晰可见。
10. 作为键盘用户，我希望 disclosure button 仍是目录展开与折叠的标准操作入口，以便新增图标不会改变现有交互。
11. 作为 Review 阅读者，我希望文件类型图标与 File Change 状态图标分别位于文件名两侧，以便不会混淆“它是什么文件”和“它发生了什么变化”。
12. 作为 Review 阅读者，我希望新增、删除、修改和重命名继续使用已有状态颜色、Tooltip 与可访问名称，以便新增类型图标不会削弱变更语义。
13. 作为 Review 阅读者，我希望当前选中 File Change 的 Diff 标题显示与文件树相同的 File Type Icon，以便导航区与内容区保持一致。
14. 作为 Review 阅读者，我希望重命名的 File Change 只按新路径显示一个类型图标，以便文件树表达 Head 中的当前结构且不会挤压路径文字。
15. 作为 Review 阅读者，我希望重命名文字继续显示旧路径到新路径的关系，以便单个类型图标不会丢失重命名信息。
16. 作为使用浅色主题的阅读者，我希望图标自动使用适合浅色背景的主题变体，以便图标保持清晰。
17. 作为使用深色主题的阅读者，我希望图标自动使用默认深色主题资产，以便无需手动配置即可保持清晰。
18. 作为离线使用桌面应用的阅读者，我希望所有可用图标随安装包提供，以便图标不依赖 CDN 或运行时网络访问。
19. 作为使用屏幕阅读器的阅读者，我希望装饰性的 File Type Icon 不重复朗读相邻文件名，以便文件树保持简洁可理解。
20. 作为窄窗口用户，我希望新增图标不破坏路径截断、选中高亮或现有 28px 紧凑行高，以便最小支持窗口仍可使用。
21. 作为维护者，我希望文件名、扩展名、目录名和主题解析集中在确定性 resolver 中，以便升级主题或排查错误时有单一规则来源。
22. 作为维护者，我希望生产构建能枚举 resolver 可能返回的全部图标，以便确认安装包不存在运行时缺失资产。
23. 作为维护者，我希望构建输出去重后的图标数量和原始总大小，以便观察依赖升级造成的资源变化。
24. 作为维护者，我希望图标资源超过 2 MiB 时收到警告但构建继续，以便评估体积而不牺牲完整类型覆盖。
25. 作为维护者，我希望 Material Icon Theme 的许可证声明随分发产物得到保留，以便满足第三方资产许可要求。

## Implementation Decisions

- 使用 `material-icon-theme` 作为文件、目录、展开目录和主题 override 的图标与关联数据源；Hugeicons 继续用于 UI 操作和 File Change 状态。
- 新增一个前端 File Type Icon resolver。输入仅为路径、节点种类、目录展开状态和当前明暗主题，输出为构建期已知的本地图标资产；不读取文件内容、真实文件系统或网络。
- 文件匹配优先级为精确文件名、最长复合扩展名、较短扩展名、通用文件 fallback。匹配规则必须确定性处理无扩展名、dotfile、多点文件名和大小写。
- 目录优先按名称匹配专属图标，再回退通用目录图标；展开与折叠分别解析。现有 disclosure arrow 保持独立，不由目录图标替代。
- 重命名的 File Change 使用新路径解析一个 File Type Icon；旧路径仍仅由现有 `oldPath → path` 文字表达。
- 文件树文件行在名称左侧显示约 16px、不可收缩的彩色 File Type Icon，右侧保留现有 File Change 状态图标。
- 文件树目录行依次显示 disclosure arrow、目录图标和目录名。新增图标不得扩大既有紧凑行高或改变整行点击行为。
- Diff 标题复用文件树的 resolver 和主题选择，显示相同 File Type Icon，并保留 File Change 状态图标。
- File Type Icon 是装饰性信息：SVG 使用 `aria-hidden`，图片使用空替代文字。文件名、`aria-expanded`、`aria-current` 以及 File Change 状态的本地化名称继续提供语义。
- 图标自动跟随现有应用明暗主题。生产资产同时包含默认主题和 light override 的全部可达图标；没有专用 override 时使用默认资产。
- 首版采用固定主题配置，不提供用户级、仓库级或运行时自定义图标关联，也不新增文件图标主题设置。
- 生产构建从固定 manifest 收集文件名、扩展名、目录名、展开目录、fallback 和明暗主题 override 的全部映射目标，对名称去重后生成静态资产映射。
- 构建必须校验每个可达图标名称均有对应 SVG。任一资产缺失属于构建错误，不能降级为运行时网络加载。
- 所有可达图标都进入 Vite/Tauri 本地产物，不依据当前开发仓库中出现的路径裁剪，也不依赖运行时访问 `node_modules` 或 CDN。
- 构建报告可达 SVG 的去重数量与原始总字节数。总量超过 2 MiB 时输出明确警告，但不删除图标、不改变 resolver 覆盖、不让构建失败。
- 保留并分发 Material Icon Theme 的 MIT 许可证声明。
- 本功能只改变 Review Reader 的前端展示和构建资产，不改变 Review、Review Snapshot、File Change 数据模型、Rust 命令、SQLite 或 Review Workspace。

## Testing Decisions

- 测试观察用户可见行为、解析契约和生产构建结果，不断言 React 内部 state、Tailwind class、SVG 内部 path 或第三方 manifest 的私有结构。
- 最高测试 seam 是渲染 Review Reader 后观察文件树与 Diff 标题：验证文件/目录图标存在、File Change 状态仍可理解、选择行为不变、重命名只显示新路径类型，以及 disclosure button 仍控制目录展开状态。
- 复用现有 Review Reader / App React Testing Library 测试方式，扩展代表性 Review Snapshot 数据；不为此功能新增 Rust 或 Tauri 命令 seam。
- resolver 使用独立表格测试覆盖精确文件名、大小写、无扩展名、dotfile、多点文件名、最长复合扩展名、未知扩展名、重命名新路径、已知/未知目录、展开/折叠和明暗主题。
- 构建资产使用一个集成 seam：从固定主题配置生成完整可达集合，断言名称去重、每个目标均有本地 SVG、默认与 light override 都被包含，并验证输出的数量与字节数统计。
- 构建测试验证超过 2 MiB 时仅产生警告且构建成功；资产缺失时构建失败。
- 生产构建验收验证应用断网后不发起远程图标请求，所有图标仍可显示。
- 按仓库前端可视化验收规则使用通用 Preview Harness 真实渲染检查。至少覆盖宽/窄窗口、长短路径、已知/未知类型、dotfile、复合扩展名、目录展开/折叠、选中/hover、四种 File Change 状态以及明暗主题。
- 视觉检查同时观察截图、可访问 DOM 和关键元素实际尺寸，确认图标保持约 16px、路径仍正确截断、28px 紧凑行高不被撑开，结束后恢复默认 viewport。
- 常规质量门槛继续包括单元测试、类型检查、Lint 和生产构建；浏览器验收不能替代这些检查。

## Out of Scope

- 用户或仓库自定义文件名、扩展名、目录名或 SVG 关联。
- 用户选择或安装其他文件图标主题。
- 从文件内容、MIME 类型、语言服务或真实文件系统探测文件类型。
- 为重命名同时显示旧类型和新类型两个图标。
- 用 File Type Icon 替换 File Change 状态、状态颜色、Tooltip 或可访问名称。
- 改变文件树的搜索、筛选、已读、审核或批准能力。
- 改变文件树的 ARIA 模型、目录 disclosure 交互或产品级键盘快捷键。
- 修改 Review、Review Snapshot、File Change、Review Workspace 或持久化模型。
- 高对比度专用应用主题；首版只覆盖 Narrant 当前的明暗主题。
- 因资源超过 2 MiB 自动裁剪图标、降低覆盖率或阻断构建。

## Further Notes

- `material-icon-theme` 的 npm 解压体积不是最终 Vite 或 Tauri 产物增量；2 MiB 门槛以构建脚本统计的可达 SVG 原始总字节数为准。
- 构建期“可达图标”指固定 resolver 在任意未来仓库路径上可能返回的完整资产闭包，而不是当前 Narrant 仓库恰好出现的文件类型。
- 若上游主题升级改变 manifest 或资产集合，维护者应通过 resolver 契约测试、资产闭包校验和体积报告显式评估变化。
- 这项选择可通过替换前端数据源与 resolver 回滚，不满足难以逆转的架构决策门槛，因此无需单独新增 ADR。

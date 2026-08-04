# Review Reader 文件类型图标方案调研

## 结论

推荐先做一个小型实现 spike，采用 [`material-icon-theme`](https://www.npmjs.com/package/material-icon-theme) 作为图标与关联数据源，在 Narrant 内实现一层纯函数 resolver，并在构建时计算和打包 resolver 可能返回的完整 SVG 闭包。它同时覆盖精确文件名、扩展名、目录名、目录展开状态和明暗/高对比主题；官方 npm 包明确面向 Web 项目提供 SVG 和 manifest 生成 API，且持续发布。与 Narrant 当前结构相比，它应只负责文件/目录的“类型”，现有 Hugeicons 继续负责变更状态和操作图标。

不建议新采用 `file-icons-js`；不建议把 Simple Icons 当作通用文件图标库；也不建议仅靠 Hugeicons 长期维护大规模扩展名映射。若 spike 证明 Material Icon Theme 的资产裁剪或 resolver 复杂度不合算，次选才是从 `vscode-icons` 衍生的现成 resolver 包。

## 已确认的产品决策

- 文件树目录行在 disclosure arrow 后显示目录图标；已知目录使用专属图标，未知目录使用通用图标，并区分展开与折叠状态。
- 文件行左侧显示约 `16px` 的 Material Icon Theme 原生彩色 **File Type Icon**，右侧继续显示现有 Hugeicons File Change 状态图标。
- Diff 标题使用与文件树相同的 File Type Icon；类型图标不替代 File Change 状态。
- 重命名的 File Change 只按新路径 `change.path` 解析并显示一个类型图标。
- 图标自动跟随 Narrant 的明暗主题；默认主题和 light override 的可达资产都随应用离线打包，不新增文件图标主题设置。
- 首版不支持用户或仓库自定义图标关联；未知文件类型使用通用文件图标。
- 生产构建打包 resolver 可能返回的全部图标，不因体积裁剪。构建输出图标数量和原始总字节数，超过 `2 MiB` 时警告但不失败。

## 当前约束

当前 [`review-reader.tsx`](../../src/components/review-reader.tsx) 将 File Change 投影成目录树：目录行只有折叠箭头，文件行右侧使用 Hugeicons 表达 added、modified、deleted、renamed。项目通过统一的 [`Icon`](../../src/components/ui/icon.tsx) 组件渲染 Hugeicons。新增图标应满足：

- React 18 + Vite 6 前端，可随 Tauri 安装包完全离线运行；
- 输入只有变更路径和目录的展开状态，不应依赖文件内容或在线探测；
- 文件类型图标位于文件名左侧，变更状态仍位于右侧，避免两种语义争夺同一个位置；
- 目录图标不能取代现有可操作的 disclosure arrow；
- 图标只辅助视觉检索，文件名、`aria-expanded`、`aria-current` 和已有本地化状态名称仍是语义来源。

## 方案比较

| 方案 | 文件匹配 | 目录/主题 | 离线与资产 | 维护与许可 | 判断 |
| --- | --- | --- | --- | --- | --- |
| `material-icon-theme` | manifest 支持 `fileNames`、`fileExtensions`、`languageIds` 和默认文件图标 | 支持目录名、展开态、根目录、light、high contrast | SVG 在 npm 包内；需显式纳入 Vite 构建并衡量裁剪结果 | MIT；近期持续发布 | **推荐** |
| `vscode-icons` 衍生 resolver | 通常直接提供按文件名解析 API | 可提供普通/展开目录；主题能力依具体 wrapper | 可离线，但部分 wrapper 会携带整套 SVG | 主题本体活跃且 MIT；Web resolver 多为第三方同步层 | 次选 |
| `file-icons-js` | `getClass(filename)` / `getClassWithColor(filename)` | 官方说明未提供目录、展开态或主题 resolver | CSS + icon font，可离线，npm 解压约 0.54 MB | MIT；1.1.0，npm 显示约六年未发布 | 不建议新采用 |
| Simple Icons | 无文件名/扩展名 resolver | 无通用目录或展开态 | 可 ESM named import，但全集 npm 解压约 16.1 MB | 包为 CC0-1.0；单个品牌仍有商标/许可提示 | 仅可少量补充品牌，不能做主方案 |
| Hugeicons 手工映射 | 需要 Narrant 自建并维护映射 | 有通用 file/folder 图形，但缺少成熟的开发文件生态映射 | 已安装，新增依赖和资产最少，可 tree-shake | 当前依赖和视觉语言一致 | 适合 fallback 或极小 MVP，不适合完整覆盖 |

npm 解压体积来自 2026-08-03 的 [npm registry 元数据](https://registry.npmjs.org/material-icon-theme/latest)：`material-icon-theme` 5.37.0 约 1.67 MB / 1,387 files，`file-icons-js` 1.1.0 约 0.54 MB / 13 files，Simple Icons 16.28.0 约 16.06 MB / 3,471 files。解压体积不是最终 bundle 体积；最终判断必须基于 Narrant 的生产构建产物。

## 为什么推荐 Material Icon Theme

该项目的[官方仓库](https://github.com/material-extensions/vscode-material-icon-theme)明确提供文件图标、目录图标、目录主题、文件/目录自定义关联、颜色、透明度和饱和度，并把项目描述为 actively maintained。其[官方 npm 包](https://www.npmjs.com/package/material-icon-theme)说明：全部 SVG 位于包内的 `icons` 目录，`generateManifest` 可生成文件名和目录名到图标的映射，因此运行时无需网络。

manifest 的[官方类型定义](https://app.unpkg.com/material-icon-theme@5.27.0/files/dist/types/core/models/manifest.d.ts)包含 `file`、`folder`、`folderExpanded`、`folderNames`、`folderNamesExpanded`、`fileExtensions`、`fileNames`、`languageIds`、`iconDefinitions`、`light` 和 `highContrast`。这比手工扩展名表更重要：`Dockerfile`、`package.json`、`.gitignore`、`foo.test.ts` 等精确文件名或复合后缀不能可靠地用“最后一个点之后”判断。

代价是它不直接提供 React 的 `resolve(path)` 组件。Narrant 需要一层确定性 resolver，建议规则至少是：

1. 规范化为 `/` 分隔符，但保留文件名大小写信息；
2. 精确文件名匹配优先；
3. 再匹配最长复合扩展名，最后才匹配短扩展名；
4. 无命中时使用 manifest 的默认 file；
5. 目录按名称与展开状态匹配，未命中时用默认 folder / folderExpanded；
6. 明暗主题只选择对应 manifest override，不改变匹配语义。

resolver 应是独立纯函数并做表格测试，至少覆盖无扩展名、dotfile、多点文件名、大小写、重命名路径、未知扩展名和目录展开态。Review 文件行应按 `change.path` 的新名称显示类型图标；旧路径仍由现有 `oldPath → path` 文本表达，避免同时显示两个类型图标。

## Vite / Tauri 打包策略

不能在运行时用任意字符串拼 `node_modules/...svg` URL。Vite 官方说明，静态 import 会转换成带 hash 的生产 URL；动态 `new URL(variable, import.meta.url)` 若无法静态分析可能不会被转换。[Vite 静态资产文档](https://vite.dev/guide/assets.html)

建议 spike 比较两条路线：

- 构建时从固定主题配置生成 manifest，收集 resolver 所有映射目标、fallback 和主题 override，生成 `iconName -> imported URL` 的 TypeScript map，并复制该集合对应的全部 SVG；这是推荐的生产路线；
- 用 `import.meta.glob(..., { eager: true, query: '?url', import: 'default' })` 纳入图标目录，先求实现简单，再检查产物。Vite 官方支持 glob import 和用 query 把资产作为 URL 导入。[Vite Features](https://vite.dev/guide/features.html#glob-import)

因为 resolver 的规则、manifest 和主题配置在生产构建时都是固定输入，构建脚本可以完全确定其可能返回的图标闭包；这不是根据当前仓库内容做猜测。构建必须校验闭包中的每个图标都有对应资产，并输出去重后的 SVG 数量与总字节数。Narrant 支持明暗主题，因此默认主题和 light override 的全部可达图标都必须进入安装包。所有可达图标都进入安装包，不因体积阈值而裁剪；首版在图标资源原始总量超过 `2 MiB` 时发出构建警告，由维护者显式评估依赖升级带来的资产增长，但警告不阻断构建。

不要假定 tree-shaking 会自动删除“目录里但未静态引用”的 SVG。验收应记录 `pnpm build` 前后 `dist` 总体积、SVG 文件数、JS 增量和首屏是否引入额外异步请求。Tauri 最终加载的是本地 Vite 产物，因此只要 SVG 进入构建输出便可离线；不要用 CDN。

## 其他候选的具体判断

### vscode-icons

应区分 [`vscode-icons/vscode-icons`](https://github.com/vscode-icons/vscode-icons) 文件主题和 [`microsoft/vscode-icons`](https://github.com/microsoft/vscode-icons) 产品 UI 图标仓库；后者不是开发文件类型主题。

主题本体活跃且为 MIT，但 Web 中常见的 resolver 是第三方同步层。例如 [`@crabnebula/file-icons`](https://www.npmjs.com/package/@crabnebula/file-icons) 提供 `getIconForFile`、`getIconForFolder`，npm 解压约 3.52 MB / 1,017 files，许可为 MIT OR Apache-2.0；其 npm 页面也明确说匹配启发式是对 vscode-icons 的简化，可能不如原主题完整。它的 API 比 Material Icon Theme 薄 resolver 简单，但版本仅 0.1.0，且把同步完整性托付给中间包，因此更适合作为备选 spike，而非默认选择。

另一类 `vscode-icons-js` wrapper 提供 `getIconForFile`、`getIconForFolder`、`getIconForOpenFolder`，但同样不是活跃主题本体的官方 Web API。采用前必须锁定具体包、核对其资产来源与同步频率，不能只写“使用 vscode-icons”。

### file-icons-js

其[官方 npm 页面](https://www.npmjs.com/package/file-icons-js)显示 API 返回 CSS class，渲染依赖配套 CSS/icon font；只介绍 filename 匹配与可选颜色 class，没有目录和展开目录匹配。它零依赖、MIT、资产不大，但发布停滞且技术形态与当前 React + SVG 图标体系不一致。

### Simple Icons

其[官方 npm 包](https://www.npmjs.com/package/simple-icons)是 3,400+ 品牌 SVG，可按 named export tree-shake，但没有文件名、扩展名、目录或 fallback resolver。因此它无法独立满足文件树需求。官方[法律免责声明](https://github.com/simple-icons/simple-icons/blob/develop/DISCLAIMER.md)还要求查看每个品牌的许可和品牌指南；即使包本身是 CC0，也不应把品牌图标许可风险视为自动清零。

### 维持 Hugeicons 手工映射

它与当前组件体系最一致，也没有新增依赖。可以作为低风险 MVP：选 10–15 个最高频类别（TS/JS、Rust、JSON、Markdown、CSS、HTML、图片、配置、测试、通用文件）和通用 folder/open-folder，其余 fallback。但若目标是 VS Code 级熟悉度，映射表、复合后缀、特殊文件名、颜色和目录语义最终都会变成 Narrant 自己的长期维护负担。

## 视觉与无障碍约束

- 文件类型图标放在文件名左侧，固定约 `size-4 shrink-0`；变更状态图标继续在右侧，保留颜色、Tooltip 和本地化状态名。
- 目录行保持 disclosure arrow 为首个可操作提示，在其后显示 folder/open-folder 图标；不要让文件夹图标承担展开按钮的 accessible name。
- 文件类型已由相邻文件名表达，应作为装饰：SVG 用 `aria-hidden="true"`，`img` 用 `alt=""`。W3C WAI 明确建议相邻文本已表达内容的装饰图片使用空 alt，避免屏幕阅读器重复或读出文件名。[WAI Decorative Images](https://www.w3.org/WAI/tutorials/images/decorative/)
- 不把颜色作为唯一分类通道；未知类型必须回退为通用文件图标，图标加载失败不能隐藏文件名或改变点击区域。
- 彩色主题图标在 selected、hover、light、dark 和 muted 背景上都要检查；不要用 CSS `currentColor` 强行覆盖多色 SVG，除非产品明确决定采用单色模式。
- 新增图标后仍需保持 28px 行高内的命中区域和截断行为，并按仓库规则使用 Preview Harness 在宽/窄窗口、长短路径、目录展开/折叠和各变更状态下做真实渲染检查。

## 建议的 spike 与决策门槛

1. 用 `material-icon-theme` 实现 resolver 原型和 20–30 个代表路径的单元测试。
2. 在通用 Preview Harness 添加长短路径、未知类型、dotfile、多点后缀、常见目录、展开/折叠、light/dark 场景。
3. 对比“全集 glob”和“构建时可达闭包 map”的生产产物；记录体积与文件数，并验证闭包零缺失。
4. 若匹配测试稳定，则采用 Material Icon Theme；打包明暗主题的全部可达图标、保留 MIT notice，并在图标资源原始总量超过 `2 MiB` 时发出不阻断构建的警告。
5. 若集成成本明显过高，则并行测一次 `@crabnebula/file-icons` 的匹配准确度和产物；仍不合算时退回 Hugeicons 小型白名单，而不是引入 `file-icons-js`。

这次调研不建议立刻把 icon pack 选择暴露为用户设置。Review Reader 的首要目标是稳定视觉检索；先固定一套主题、把离线资产、匹配规则和无障碍做好，再依据真实反馈决定是否增加主题配置。

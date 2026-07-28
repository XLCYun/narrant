# Narrant 前端国际化方案调研

日期：2026-07-28

## 结论

Narrant 可以做 i18n，且不需要改动现有 Rust 领域模型或 SQLite 数据模型。推荐采用：

- `i18next` + `react-i18next`
- 中文简体 `zh-CN` 作为第一版默认语言和回退语言
- 英文 `en` 作为第二种语言
- Tauri 官方 `@tauri-apps/plugin-os` 读取系统 locale
- 语言偏好先用 WebView 的 `localStorage` 保存；如果以后需要跨 WebView/窗口共享，再切换到 Tauri 官方 Store 插件
- 翻译资源随应用打包，通过 Vite 动态 `import()` 按语言和 namespace 加载，不依赖网络
- 日期、数字和相对时间直接使用浏览器原生 `Intl`

当前项目是 React 18.3、Vite 6、TypeScript 5.7 和 Tauri 2。`react-i18next` 官方仓库要求 React/React DOM >= 16.8，因此适配当前 React 18；i18next 当前类型系统要求 TypeScript 5，亦适配项目的 TypeScript 5.7。[react-i18next 官方仓库的 requirements](https://github.com/i18next/react-i18next#requirements)；[i18next TypeScript 文档](https://www.i18next.com/overview/typescript)

实施时建议安装当时最新的兼容版本，并让 pnpm lockfile 固定实际版本；以当前时间点可使用 i18next 26.x。不要在方案文档中锁死 react-i18next 的未来小版本。

## 当前项目现状

`package.json` 目前没有 i18n、locale 检测或偏好存储依赖；`src/main.tsx` 直接渲染 `App`，`src/App.tsx` 中大量中英文界面字符串内联。测试使用 Vitest、Testing Library 和 WDIO/Tauri，适合分层增加国际化测试。

当前 UI 尚在整体重构前，正是引入稳定 message key 的合适时机。否则先重写页面、再从 JSX 中搬运文案，会产生重复工作。

## 候选方案比较

| 维度 | i18next + react-i18next | FormatJS + react-intl |
|---|---|---|
| React 集成 | `useTranslation`、`Trans`、Provider；hook 在资源未就绪时可使用 Suspense | `IntlProvider`、`useIntl`、`FormattedMessage` 等组件 |
| 翻译组织 | 原生支持 namespace、语言/namespace fallback、动态加载 | 通常按 locale 向 Provider 提供 messages；代码分包需应用自行编排 |
| 消息语法 | 简单 key/value、插值、复数；可扩展 | ICU Message Syntax 是核心，复数、select 和富文本表达力强 |
| 日期/相对时间 | 可配置 formatter，也可直接用原生 `Intl` | 内置 `FormattedDate`、`FormattedRelativeTime` 和 imperative API |
| 工具链 | 官方 i18next CLI 涵盖抽取、类型、同步和 lint；也可先不引入 CLI | 官方 CLI、Babel 插件/TS transformer 的抽取与预编译工作流更成熟 |
| TypeScript | 可通过 module augmentation 和资源类型实现 key 类型安全；v25.4 起有 selector API | TypeScript 原生支持；message descriptor 与抽取链路清晰 |
| Narrant 适配度 | 更符合按页面/领域 namespace、离线 JSON/TS 资源、渐进迁移 | 适合一开始就采用 ICU message-first 与严格抽取流水线的团队 |

依据：

- i18next 将 namespace 定义为可拆分、可按需加载的翻译集合，并明确列出减少初始加载和按语义拆分的用途。[i18next Namespaces](https://www.i18next.com/principles/namespaces)
- i18next 原生提供语言变体、语言和 namespace fallback。[i18next Fallback](https://www.i18next.com/principles/fallback)
- `useTranslation` 可绑定 namespace；资源未就绪时默认触发 Suspense，也可关闭 Suspense 后自行处理 ready 状态。[react-i18next useTranslation](https://react.i18next.com/latest/usetranslation-hook)
- FormatJS 以 ICU Message Format 为消息核心，提供日期、数字、相对时间和富文本格式化。[React Intl API](https://formatjs.github.io/docs/react-intl/api/)
- FormatJS 官方工作流强调 message 声明、CLI 抽取、翻译和编译；其 CLI 能保留 `description` 供翻译者理解上下文。[FormatJS Application Workflow](https://formatjs.github.io/docs/getting-started/application-workflow)、[Message Extraction](https://formatjs.github.io/docs/getting-started/message-extraction/)

### 为什么推荐 i18next

Narrant 当前是体量较小、中文优先、无翻译供应商流水线的桌面应用。近期的首要任务是将导航、仓库、PR、审核阅读器、错误状态等界面文案从组件中解耦，而不是建立 ICU 抽取/编译基础设施。i18next 的 namespace、fallback 和资源加载接口能以较小改造覆盖这一目标。

建议 namespace：

- `common`：通用按钮、状态、错误动作
- `catalog`：首页、仓库侧边栏、仓库概览
- `review`：文件树、Diff、版本更新、Review 概览
- `github`：连接、PR 发现、授权

第一阶段只预加载 `common` 和首屏需要的 `catalog`，进入 Review 时加载 `review`，进入 GitHub 流程时加载 `github`。i18next 提供 `loadNamespaces`，且 namespace 本身就是官方推荐的按需加载边界。[i18next API](https://www.i18next.com/overview/api)、[Namespaces](https://www.i18next.com/principles/namespaces)

FormatJS 并非不可用；如果未来形成专业翻译团队、要求所有默认消息自动抽取、广泛使用复杂 ICU plural/select，届时可重新评估。但以当前产品阶段采用它会让 Vite 的抽取/编译配置先于真实需求到来。

## locale 检测与偏好

语言解析优先级应固定为：

1. 用户显式选择并保存的应用语言；
2. Tauri 系统 locale；
3. `zh-CN` 回退。

Tauri 2 官方 OS 插件的 `locale()` 返回 BCP-47 language tag，无法获取时返回 `null`，比依赖 WebView 的 `navigator.language` 更准确地表达“桌面系统语言”。[Tauri `locale()` API](https://v2.tauri.app/reference/javascript/os/#locale)

只支持 `zh-CN` 与 `en` 时，应显式归一化：

- `zh`、`zh-CN`、`zh-Hans-*` → `zh-CN`
- `en`、`en-*` → `en`
- 其他语言 → `zh-CN`

不要将未经校验的 locale 直接拼入动态 import 路径。使用白名单映射，避免未知路径和无意义的加载失败。语言标识使用 BCP-47，i18next 也建议采用该规范。[i18next FAQ](https://www.i18next.com/how-to/faq)

### 持久化选择

第一版使用 `localStorage` 保存 `narrant.ui.locale` 即可：

- 偏好是纯前端、单窗口、非敏感数据；
- 与侧边栏宽度、Diff 布局等本地 UI 偏好一致；
- 无需 Rust command、数据库 migration 或新 Tauri capability。

如果未来出现多窗口同步、WebView 存储迁移风险或希望统一管理桌面偏好，改用 Tauri Store 插件。官方 Store 将文件保存在 `app_data_dir`，提供 `load/get/set/save` 及变更监听。[Tauri Store API](https://v2.tauri.app/reference/javascript/store/)

语言选择器不是第一版 UI 重构的阻塞项。可以先按系统语言启动，并在将来设置入口加入“跟随系统 / 简体中文 / English”；选择“跟随系统”时删除显式偏好，而不是把当前系统语言永久写死。

## 翻译资源加载

桌面应用应把翻译资源打入安装包，不能以 HTTP backend 从远程获取，否则离线启动会出现缺字或加载失败。

推荐结构：

```text
src/i18n/
  index.ts
  locale.ts
  resources/
    zh-CN/common.ts
    zh-CN/catalog.ts
    zh-CN/review.ts
    zh-CN/github.ts
    en/common.ts
    en/catalog.ts
    en/review.ts
    en/github.ts
```

用显式 loader map 包装 Vite 动态 import：

```ts
const loaders = {
  "zh-CN": {
    common: () => import("./resources/zh-CN/common"),
    catalog: () => import("./resources/zh-CN/catalog"),
  },
  en: {
    common: () => import("./resources/en/common"),
    catalog: () => import("./resources/en/catalog"),
  },
} as const;
```

首屏初始化完成前显示现有启动底色/轻量 skeleton，避免先闪现 key 或错误语言。页面级 namespace 可由路由/视图切换函数显式预加载；若直接依靠 `useTranslation`，在相应区域加 Suspense boundary，不要让整个应用因一个 namespace 挂起。react-i18next 官方说明了该 hook 的 Suspense 行为。[useTranslation](https://react.i18next.com/latest/usetranslation-hook)

## 日期和相对时间

使用当前解析后的 locale 创建集中式 formatter：

- 精确时间：`Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" })`
- 最近打开时间：先把时间差换算成合适 unit，再使用 `Intl.RelativeTimeFormat(locale, { numeric: "auto" })`
- Tooltip/详情展示绝对时间，列表展示“3 分钟前”或“3 minutes ago”

`Intl.RelativeTimeFormat` 是 ECMA-402 标准能力。[ECMA-402 RelativeTimeFormat](https://tc39.es/ecma402/2024/#relativetimeformat-objects) React Intl 也基于同一个 Intl 能力提供 `formatRelativeTime`，但为了相对时间单独引入 react-intl 没有价值。[React Intl API](https://formatjs.github.io/docs/react-intl/api/#formatrelativetime)

相对时间需要定时更新时，只在“最近审核”列表挂一个共享的低频 tick（例如每分钟），不要每行创建 timer；秒级精度对该场景没有产品价值。

## 消息设计规则

- key 表达语义，不以中文原文作为 key，例如 `review.update.checkFailed`。
- 用户界面中文为主；`Pull Request`、`Diff`、`Base`、`Head`、`GitHub` 等产品约定术语可保留英文。
- 不拼接句子。变量使用插值，数量使用 i18next plural。
- ARIA label、tooltip、空状态、Toast、Dialog 和错误恢复按钮全部进入资源文件，不能只翻译可见标题。
- Git/Rust 原始错误作为技术详情；面向用户的摘要使用可翻译的错误类别和参数。
- `zh-CN` 是源语言与 `fallbackLng`；生产环境禁止开启 `saveMissing`。
- 启用 `supportedLngs: ["zh-CN", "en"]`，并使用 `resolvedLanguage` 驱动 `document.documentElement.lang`；若以后支持 RTL，再以 `i18next.dir()` 设置 `dir`。[i18next configuration](https://www.i18next.com/overview/configuration-options)、[i18next API](https://www.i18next.com/overview/api)

## 类型安全与版本注意事项

建议资源先写成 TypeScript `as const`，再通过 i18next module augmentation 将 `zh-CN` 资源声明为基准类型。这能在编译期发现错误 key 和错误插值参数。官方文档指出 JSON 资源无法直接获得 `as const` 的字面量推断；当前项目使用 TS 资源更合适。[i18next TypeScript](https://www.i18next.com/overview/typescript)

i18next 25.4 引入 `enableSelector`，26 计划默认启用 selector API。若直接采用 i18next 26，应按该版本官方迁移说明决定使用 selector key 还是暂时显式配置；不要复制旧教程的默认行为。项目的 TypeScript 5.7 满足 i18next 最新版要求，但启用前应在 lockfile 中实际验证 `pnpm build`。[i18next TypeScript 版本说明](https://www.i18next.com/overview/typescript)

react-i18next 的官方最低要求远低于当前 React 18.3，因此不存在 React 基线冲突。[react-i18next requirements](https://github.com/i18next/react-i18next#requirements)

## 测试策略

### 单元测试

- `normalizeLocale` 表驱动测试：`zh-Hans-CN`、`zh-TW`、`en-US`、未知语言、`null`。
- 偏好优先级测试：已保存偏好覆盖系统语言；清除偏好恢复系统语言。
- 日期/相对时间测试固定 locale、固定 timestamp 和时区；不要依赖运行测试机器的 locale。
- namespace loader 测试未知语言/namespace 不会构造任意路径。

### 组件测试

建立 `renderWithI18n`，每个测试创建独立 i18next instance，注入同步内存资源并指定 locale，避免全局实例在 Vitest 并行测试间泄漏。

- 现有以中文或英文 accessible name 查询的测试应显式使用 `zh-CN`。
- 至少为首页、仓库空状态、Review 更新 Toast 各加一个英文 smoke test。
- 用 `cimode` 测试关键页面没有硬编码文案；i18next 官方说明 `cimode` 会直接返回 key，适合测试。[i18next API `changeLanguage`](https://www.i18next.com/overview/api#changelanguage)
- 开启 key 类型检查后，`pnpm build` 同时承担 message key 静态检查。

### E2E

- 在测试启动时通过专用测试入口固定语言，不依赖 CI 主机系统 locale。
- 一条用例验证无偏好时系统 locale 映射；一条验证用户偏好在应用重启后保留。
- 截图/文本断言分别覆盖 `zh-CN` 与 `en` 的核心路径，不需要把整个 E2E 套件翻倍。

## 分阶段实施

### 阶段 1：基础设施与首页垂直切片

1. 安装 `i18next`、`react-i18next`、Tauri OS plugin，并按 Tauri 2 要求注册插件与 capability。
2. 实现 locale 归一化、偏好解析、i18n 启动和 `renderWithI18n`。
3. 建立 `common`、`catalog` 的中英文资源。
4. 迁移首次使用空状态、侧边栏、最近审核列表；包括 ARIA 与 Toast。
5. 增加 locale、加载器、中文/英文组件测试。

验收：离线启动正常；简中系统显示中文；英文系统显示英文；未知系统语言回退中文；首页不再有未登记的用户文案。

### 阶段 2：完整前端迁移

1. 依次迁移 `review`、`github` namespace。
2. 统一绝对时间和相对时间 formatter。
3. 将 Git/Rust 错误映射为稳定、可翻译的前端错误码/类别；技术详情保留原文。
4. 添加 namespace 懒加载与局部 Suspense/skeleton。
5. CI 增加 key 完整性检查和英文 smoke tests。

验收：核心用户路径中没有硬编码界面文案；切换语言不会重建 Review 或触发后端领域操作；懒加载失败有中文/英文可恢复提示。

### 阶段 3：用户语言设置与翻译工作流

1. 有真实设置入口后加入“跟随系统 / 简体中文 / English”。
2. 评估是否从 `localStorage` 升级到 Tauri Store，以多窗口/统一偏好需求为准。
3. 文案规模增长后引入 i18next 官方 CLI 做 key 抽取/同步/lint；i18next 官方插件列表将其定义为覆盖抽取、类型生成、locale 同步和 lint 的工具。[i18next Plugins and Utils](https://www.i18next.com/overview/plugins-and-utils)
4. 若引入翻译供应商，再决定 JSON 导入导出或 TMS，不在第一版绑定云服务。

## 风险与边界

- 系统 locale 不是产品语言偏好的替代品；显式选择必须优先。
- `zh-TW` 不应静默映射为简体中文。第一版没有繁体资源时应按产品回退规则进入 `zh-CN`，并记录这是“资源缺失回退”，不是语言等价。
- WebView 的 Intl 能力取决于系统 WebView。首批目标平台应在 E2E 中验证 `Intl.RelativeTimeFormat`；若实际缺失，再按需添加 polyfill，不预先增加体积。
- 翻译资源属于 UI，不应进入后端持久化；Review 标题、分支名、GitHub 原始文本等用户/外部数据也不翻译。
- 本方案不要求第一版立即显示语言切换器；但现在开始使用 message keys，可避免后续 UI 重构完成后再次大规模搬迁。

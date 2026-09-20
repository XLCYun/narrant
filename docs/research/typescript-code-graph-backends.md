# TypeScript Code Graph 分析后端事实调研

日期：2026-09-14（Asia/Shanghai）。性质：候选技术的一手资料底稿；最终工程建议以主设计和验证报告为准。

本篇核验官方文档、维护者仓库、版本化源码和 npm 发布元数据，不包含性能跑分。
用户已确认默认仓库内 + 标准库，包含按环境提供的受控 DOM/Node 声明；其他产品范围也已收敛，
见 [主设计的确认记录](typescript-code-graph-plan.md#本轮已确认的范围)。本篇的候选比较不代替产品决策。
本轮本地 Compiler API、原生 LSP 和 tsserver 项目范围探针见[接入方案与验证记录](typescript-code-graph-plan.md)，不能把源码阅读当成实测。VS Code 当前稳定版的后端选择另见[VS Code 参考](typescript-vscode-reference.md)。
后续 23 个完整 TLS 会话支持推荐 TLS 6.0.0 + tsserver 6.0.3；同时发现 noResolve 不适合
通用 references 项目，推荐保留项目关系的分析目录。见 [LSP 实测](typescript-lsp-validation.md)。

## 当前版本事实先于后端选择

| 正式版本 | 正式发布日期 | 与兼容目标的关系 |
| --- | --- | --- |
| TypeScript 5.0 | 2023-03-16 | TS5 是已有三年历史的版本系列，支持旧仓库有实际意义 |
| TypeScript 6.0 | 2026-03-23 | 最后一个旧 JavaScript 实现的主要版本，官方将其定位为 5.9 与 7.0 之间的过渡 |
| TypeScript 7.0 | 2026-07-08 | 原生实现与新 LSP；不代表全部旧配置、JS 推断及 Compiler API 等价 |

以上为 GA 日期，不是 Beta 或 RC 日期。
[5.0 正式发布](https://devblogs.microsoft.com/typescript/announcing-typescript-5-0/)、
[6.0 正式发布](https://devblogs.microsoft.com/typescript/announcing-typescript-6-0/)、
[7.0 正式发布](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)

TypeScript 7.0 已于 2026-07-08 发布，是原生 Go 实现，包含 LSP。官方说明 7.0
尚未提供稳定程序 API，依赖旧 Compiler API 的工具可通过 `@typescript/typescript6`
继续使用 6.0 API。7.1 的新 API 不承诺与旧 API 相同。
[TypeScript 7.0 发布说明](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)

本次直接读取 npm 元数据确认 `typescript` 的 latest 为 **7.0.2**。其 package exports
已经包含 `./unstable/sync`、`./unstable/async`、`./unstable/ast` 等入口；因此准确说法是
“存在明确标记为 unstable 的接口，尚不能作为已稳定的嵌入契约”，不能说完全没有代码可调用。
包的根入口是版本信息，不是旧版 `createProgram` 导出。平台包覆盖 macOS、Windows、Linux
等目标；顶层 npm 启动器与直接执行平台二进制是不同交付方式。
[typescript 7.0.2 发布元数据](https://registry.npmjs.org/typescript/7.0.2)

旧 `microsoft/typescript-go` 仓库顶部已说明开发迁回 `microsoft/TypeScript`。
旧 README 下方“API not ready”的历史状态表不能代替当前发布包核验。
7.1 迭代计划仍将 API 稳定化列为工作，计划稳定发布日期为 2026-11-24，属于计划而非交付承诺。
[旧仓库 README](https://github.com/microsoft/typescript-go/blob/main/README.md)、
[TypeScript 7.1 迭代计划](https://github.com/microsoft/TypeScript/issues/63703)

下文原生 LSP 事实优先引用 **v7.0.2** 源码；旧 Compiler API 引用 **v6.0.3** 源码。
Oxc、SWC 和 Tree-sitter 的滚动文档只证明当前可用能力，进入实现时还需固定版本。

原生 compiler/checker 的 Go 源码位于 `internal` 包；Go 的 internal 导入规则限制
外部工程直接依赖它。自行 fork 或在其模块内加专用服务属于另一条维护路线，不能把
“Go 源码公开”写成已有稳定 Rust/Go 嵌入库。
[原生源码布局](https://github.com/microsoft/TypeScript/tree/main/tsc/internal)、
[Go internal 规则](https://pkg.go.dev/cmd/go#hdr-Internal_Directories)

## 对 Narrant 的约束

已有 `LanguageAnalyzer` 接收一个准备好的仓库状态、所选文件和分析选项，返回公共
Code Graph。文件选择限制图成员，语义上下文可以读取未选文件；已知图外目标使用
External，无法确定的目标使用 Unresolved。每一侧的源码和解析上下文必须与该侧固定状态
一致，两侧使用相同分析器约定；不要求前后版本的源码或配置内容相同。身份不能包含
绝对 checkout 路径。TSX 已被界定为带 JSX 语法的 TypeScript，不要求建立 HTML 图。
[语言接口](../specs/0018-structure-review-code-graph.md#language-interface-and-input)、
[对象模型](../specs/0018a-code-object-kinds.md)、
[关系约定](../specs/0018b-code-relationships.md)、
[Rust 接口](../specs/0018c-rust-interface.md)

已有 Rust 实现采用本地语法树加外部语义服务器；其中通用 `LspClient` 与 Rust 的
配置、就绪判定分开。因此原生 TypeScript LSP 可以复用传输方向，但 Rust 的
`experimental/serverStatus`、Cargo 发现和 Rust 对象规则不能原样搬过去。
[现有 Rust 分析器](../specs/0019-rust-language-analyzer.md#shared-lsp-module-and-rust-adapter)

## 候选能力比较

本表是根据下文证据作出的工程判断，不是性能排序。

| 路线 | 能直接获得的材料 | 需要 Narrant 自己补齐的工作 | 当前适用判断 |
| --- | --- | --- | --- |
| 固定 typescript-language-server + 传统 tsserver + Oxc | 官方项目服务的定义、引用、调用层级；Oxc 的语法对象；LSP 包装复用现有传输 | 项目输入和归属、稳定身份、未解析保留；Node/包装层分发和插件控制 | 本轮主方案；优先验证 TS6 服务和 TS5/6 项目兼容，以及包装层对受控外部项目的使用 |
| TypeScript 7 原生 LSP + Oxc 语法提取 | 官方项目语义、定义位置、调用层级；Rust 内的显式声明与源码范围 | 将位置映射为稳定身份；补未解析调用；过滤不同关系；旧配置兼容及默认依赖范围 | 原生服务候选；不能只靠关闭 ATA 禁止第三方进入项目，须验证实际项目输入 |
| 固定版本 TypeScript Compiler API + 随附 Node worker | 同一 AST、Symbol、TypeChecker、声明集合、别名链和调用签名；可控制文件宿主 | 自己建立项目会话、文件边界、IPC、取消、身份和图提取；承担旧 API 生命周期及兼容版本管理 | 需要更细数据访问或服务缺能力时的备选；默认依赖范围本身不构成必须采用它的理由 |
| Oxc parser + semantic + resolver 单独使用 | AST、局部作用域与绑定、标识符读写标记、模块文件解析 | 跨文件导出表、别名/合并声明、基于类型的成员和调用解析 | 可提供结构或受限解析模式，不能承诺与 TypeScript checker 同等语义 |
| SWC 单独使用 | TS/JS AST、词法绑定上下文、代码生成和模块加载工具 | 与上行相似，仍缺完整 TypeScript checker | 可作语法层候选；本次没有证据表明其比 Oxc 更适合现有需求 |
| Tree-sitter 单独使用 | TS/TSX concrete syntax tree、容错与源码范围 | 作用域、绑定、项目模型、跨文件和类型语义几乎都需另建 | 适合语法导航或容错提取，不能独立承担本次完整语义后端 |

## TypeScript 7 原生 LSP 的已核验能力

v7.0.2 服务端初始化声明 `definitionProvider`、`typeDefinitionProvider`、
`implementationProvider`、`documentSymbolProvider`、`callHierarchyProvider`，
并注册 prepare/incoming/outgoing call hierarchy。该版本没有声明 TypeHierarchyProvider。
位置默认 UTF-16，客户端声明 UTF-8 时会选 UTF-8。
[v7.0.2 LSP server](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/internal/lsp/server.go)

这使“通过语法识别 extends/implements，再查询相应名字的定义”成为可验证路径；
不能假设有标准 type hierarchy 可直接枚举继承关系。documentSymbol 适合辅助导航，
不替代 Narrant 的六类对象、own content、单一声明位置和内容归属规则。

v7.0.2 的调用层级实现会沿符号及 import alias 找值声明，识别命名函数、方法、
某些绑定到 const 的箭头/函数表达式。它也收集 JSX 标签、new、tagged template、
decorator 和属性访问。找不到声明的调用直接被省略。结果是名字、URI、range、
selectionRange 和容器说明，不提供可持久复用的 symbol ID。
[v7.0.2 call hierarchy](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/internal/ls/callhierarchy.go)

**推论**：Oxc 必须保留语法观测到的调用位置，并把查不到目标的调用映射为 Unresolved。
不能把 outgoingCalls 当成完整调用清单；也不能把其中的 JSX 或 accessor 观察不加区分
全部改写成普通 `code.calls`。间接变量、联合类型、动态属性和工厂返回值是否具有足够的
目标证据，须用真实包 fixture 检验。一个定义位置或签名声明并不自行证明运行时唯一目标。

### 生命周期与离线边界

原生二进制的入口为 `--lsp --stdio`。v7.0.2 的其他通信选项尚不工作；它包装了
标准库文件系统，并实现父进程存活检查。标准库可以嵌入二进制，也存在非嵌入构建，
所以独立打包须核验实际平台产物，不能只复制一个自编译 binary 后假定标准库齐全。
[CLI 入口](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/cmd/tsgo/main.go)、
[LSP 启动](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/cmd/tsgo/lsp.go)、
[bundled 文件系统](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/internal/bundled/bundled.go)

`initialized` 期间服务端配置项目 session，并可能请求 `workspace/configuration`
和 `client/registerCapability`。内部 initialization gate 与按请求创建的 session snapshot
不能等同 rust-analyzer 的 ready 通知。适配器需要按本版本实际握手建立查询顺序和超时，
并核验正常 shutdown、异常退出、取消和进程回收。
[LSP 初始化与请求处理](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/internal/lsp/server.go)

原生服务器仍有 Automatic Type Acquisition（ATA），能调用 npm 安装类型包。
安装参数包含 `--ignore-scripts`，但依然会联网并更新全局类型缓存。
要验证“不安装依赖”的模式，应显式关闭 ATA；原生并不意味着默认离线。
[ATA 实现](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/internal/project/ata/ata.go)

当前关闭方式可通过 `js/ts` 配置段的 `tsserver.automaticTypeAcquisition.enabled=false`。
当客户端不提供 workspace configuration 时，初始化 `userPreferences` 使用嵌套对象：

```json
{
  "initializationOptions": {
    "userPreferences": {
      "tsserver": {
        "automaticTypeAcquisition": { "enabled": false }
      }
    }
  }
}
```

这项统一配置优先于旧的 `disableAutomaticTypeAcquisition`。
[偏好解析与 IsATADisabled](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/internal/ls/lsutil/userpreferences.go)

### 旧仓库配置是实质门槛

**建议继续以 TS5/TS6 仓库兼容为产品目标。** 后端程序的版本与被分析仓库使用的
TypeScript 版本是两件事；选择新后端不构成要求用户升级项目的理由。这里的兼容至少分为
能解析源码、能读取项目配置、能维持符号/模块语义三层，不能只凭解析成功承诺完全等价。

TS6 已改变一些默认值：`strict` 默认开启、`rootDir` 默认配置所在目录、`types`
默认空数组。TS7 则移除
`baseUrl`、`moduleResolution: node/node10/classic` 等弃用选项，并有 JavaScript/JSDoc
推断变化。旧项目可能同时受配置错误和无错误但推断不同影响。
[TS6 默认值与迁移](https://devblogs.microsoft.com/typescript/announcing-typescript-6-0/)、
[TS7 移除选项](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)

**资料冲突**：6.0 发布说明称 `baseUrl` 已不再作隐式查找根，但 v6.0.3
`moduleNameResolver.ts` 仍调用 `tryLoadModuleUsingBaseUrl` 并组合 baseUrl 与裸导入名
执行查找。这里仅确认 TS6 弃用、TS7 移除；TS6 具体兼容行为需以固定发布包实测为准，
不能把博客的迁移措辞直接当成 v6.0.3 已删除该实现。
[v6.0.3 模块解析源码](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/compiler/moduleNameResolver.ts)

Narrant 自己的配置仍使用 `baseUrl` 加 `paths`。候选方案需要同时验证原始配置、
缺失配置和多 tsconfig；不能为使分析成功而修改正在比较的业务源码或悄悄迁移 tsconfig。
若未来选择分析专用配置覆盖，必须定义哪些语义发生变化、如何向用户说明，并保持两侧一致。

**待验证的兼容策略**：先固定受控的 TS5.9 与 TS6 兼容档，而非从仓库动态加载任意
compiler。一个 TS5.9 档也不等于逐个复刻 5.0 至 5.8 的全部旧行为。应按实际项目
语法/配置样本界定范围，记录分析版本；不支持的旧规则保留局限信息，不能改用新的
`nodenext` 或默认 strict 后还标记为原语义。TS7 的新语法同样不能假定旧 API 已支持。

### 禁止解析第三方需要控制输入

用户确认的“默认不解析第三方依赖”应落实在数据输入层。以下行为需要分开描述：

当前要求对齐 Rust 的依赖加载范围：正常项目分析中，第三方源码/声明不进入语义项目。
它不是新增任意输入下的 OS 文件访问沙箱要求。上一轮因 CompilerHost 更易控制文件而
直接提高其排序，遗漏了服务项目输入路线；该推荐已被本轮证据修正。

| 行为 | 含义与建议状态 |
| --- | --- |
| 不把依赖放进图 | 只控制输出成员，后端仍可能读取、解析并利用完整第三方 `.d.ts`，不满足默认分析范围 |
| 不解析第三方源码和类型声明 | 禁止第三方 `.ts/.js/.d.ts` 等进入语义程序；第三方返回值与成员的证据可能随之丢失 |
| 读取 `package.json` 等元数据 | 用于包身份、`type/imports/exports/types/typesVersions` 和 workspace 路由；JSON 读取不执行包脚本，建议与源码/声明读取分别授权和记录 |
| 读取运行时类型 | 随 compiler 的 ECMAScript/DOM 声明与 `@types/node` 是不同来源；Q3 已接受该范围，具体版本和必要声明闭包仍需固定 |

以下选项各有用途，但都不是选择性第三方文件沙箱：

- 关闭 ATA 只禁自动下载类型；已安装类型仍可被读取。
- `types: []` 限制全局类型包自动纳入，显式 import 仍可带入包声明。
- `skipLibCheck` 跳过声明文件的完整检查；声明仍可为应用代码提供类型。
- 顶层 `exclude: ["node_modules"]` 只过滤 `include` 搜索，import/reference/files 仍可带入文件。
- `maxNodeModuleJsDepth: 0` 限制依赖 JS 推断深度，不禁 `.d.ts`。

[types](https://www.typescriptlang.org/tsconfig/types.html)、
[skipLibCheck](https://www.typescriptlang.org/tsconfig/skipLibCheck.html)、
[顶层 exclude](https://www.typescriptlang.org/tsconfig/#exclude)、
[maxNodeModuleJsDepth](https://www.typescriptlang.org/tsconfig/maxNodeModuleJsDepth.html)

`noResolve` 确实阻止 import/reference 自动扩展 Program，但也阻止仓库内部文件的
正常闭包。v7.0.2 仍运行模块解析；标准库引用和自动类型输入另有路径。要尝试
`noResolve`，需自行预枚举第一方文件及获准的完整运行时声明，约束 `types`，并验证多
项目行为。它不是只禁止外部依赖的开关，本次未实测这种 LSP 模式。
[noResolve](https://www.typescriptlang.org/tsconfig/noResolve.html)、
[v7.0.2 文件解析](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/internal/compiler/filesparser.go)、
[v7.0.2 import 加载](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/internal/compiler/fileloader.go)

v7.0.2 CLI 固定用 `bundled.WrapFS(osvfs.FS())`。内部 `ServerOptions` 虽有 `FS`，
本次核验的公开 LSP 初始化和 CLI 没有把客户端白名单文件宿主注入进去的接口。因此，
原样对真实 checkout 启动 stock LSP 不能承诺禁止解析已安装第三方。
[LSP CLI 文件系统](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/cmd/tsgo/lsp.go)、
[内部 ServerOptions](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/internal/lsp/server.go)

**工程推论**：TS7 路线需要验证明确枚举项目文件、受控分析视图等项目输入办法。
若进一步要求任意文件读取隔离，才需要讨论更完整的物化视图、路径边界或可注入 FS 的
适配层；这些不能自动当成本次用户要求。7.x unstable API 也不能直接视为稳定替代品。
Compiler API 的 Host 在 readFile、fileExists、readDirectory、realpath、模块解析和配置
宿主上提供更细控制，但它不是唯一候选。传统 tsserver 的项目协议已经给出另一条可验证
路径，见下文和新增本地探针。无论采用哪条路线，仅限制输出图成员都不够。

## TypeScript 6 Compiler API + Node worker

官方旧 API 以 Program、CompilerHost 和 SourceFile 为核心，支持直接遍历 AST，
通过 TypeChecker 读取符号与类型，并用 printer 输出源码。CompilerHost 允许控制文件访问。
官方 wiki 已明确其说明适用于 6.0 及以前，不能据此推导 7.x API。
[Compiler API 指南](https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API)

v6.0.3 的接口包含 `getSymbolAtLocation`、`getAliasedSymbol`、`getExportsOfModule`、
`getResolvedSignature`、`getSignatureFromDeclaration`、`isImplementationOfOverload`、
`getMergedSymbol`。Symbol 可携带多份声明；错误调用也可能产生 unknown signature。
这些接口适合获取别名、重导出、重载、声明合并的材料；它们不会替 Narrant 决定怎样建图。
CompilerHost 还可提供 CancellationToken。新的模块解析模式应使用
`resolveModuleNameLiterals`，而非照搬旧 wiki 示例中的已弃用 hook。
[v6.0.3 类型与宿主接口](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/compiler/types.ts)

tsconfig 的 JSON/JSONC 读取和展开可使用官方 `readConfigFile`、
`parseJsonConfigFileContent` / `getParsedCommandLineOfConfigFile`，保留 compilerOptions、
rootNames 和 projectReferences；这比自己解释 `extends`、files、include、exclude 更可靠。
[v6.0.3 配置解析](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/compiler/commandLineParser.ts)

**建议草案**：固定 compiler 版本与随附 Node，把 AST/TypeChecker 留在 worker 内，
只通过一个受控协议返回图构建所需数据。Rust 保持公共图校验、任务取消和进程边界。
两侧使用独立 Program、独立解析缓存和固定文件宿主；不要借用用户编辑器的缓冲区。
缓存复用是之后需要独立证明正确性的优化。

一个 Program 不等于整个任意 monorepo。官方 project references 通常跨项目消费 `.d.ts`；
编辑器可用内存重定向缓解缺少构建产物，但普通 `tsc` 不会自动构建依赖。
仅向 `createProgram` 塞入全仓库文件会丢失不同项目的编译选项与上下文区别。
[Project References](https://www.typescriptlang.org/docs/handbook/project-references.html)

因此需决定按哪个 tsconfig 建 Program、如何处理一个文件属于多个项目，以及缺 `.d.ts`
时是否使用经验证的 source redirect。不能为了补类型默认执行 `pnpm install` 或构建脚本。
解析成功的 import 文件路径也不等于已完成导出符号、实例类型和成员方法解析。

### Node.js、模块解析与运行时类型

Node.js 项目可以使用 TypeScript、JavaScript，或混合两者。CommonJS 与 ESM 也都
仍受支持；不能从“TS7 改为 Go 实现”推导“不支持 Node.js 项目”。分析 worker 随附的
Node 版本是工具运行时，项目面向哪个 Node 版本则应来自项目配置，二者不能混同。

| 项目模式 | 对静态解析的影响 |
| --- | --- |
| `node16` | 对应固定一代 Node 双模块规则，并非仅 ESM；应保留项目原设定 |
| `nodenext` | 同时处理 ESM/CJS，随 TypeScript 版本跟随新的 Node 规则；固定 compiler 版本对可重复性有意义 |
| `bundler` | 模拟打包器的混合规则，支持 imports/exports，但允许 Node ESM 不接受的无扩展名相对导入 |

[moduleResolution 选项](https://www.typescriptlang.org/tsconfig/moduleResolution.html)、
[Node 模式](https://www.typescriptlang.org/docs/handbook/modules/reference.html#node16-node18-node20-nodenext)

在上述 Node 模式中，`.mts/.mjs/.d.mts` 指明 ESM，`.cts/.cjs/.d.cts` 指明 CJS；
普通 `.ts/.tsx/.js/.jsx/.d.ts` 还受最近 `package.json` 的 `type` 影响。
`exports` 决定包对外入口及条件分支，`imports` 定义包内 `#` 路由；import 与 require
可能选择不同文件。TypeScript 还会考虑 `types`、版本化类型条件等类型信息。
因此忽略 package metadata，即使只分析仓库自己的 Node 代码，也可能连到错误目标。
[Node 包格式、入口和条件](https://nodejs.org/api/packages.html)、
[TypeScript 模块解析](https://www.typescriptlang.org/docs/handbook/modules/reference.html#the-moduleresolution-compiler-option)

TypeScript 解析有扩展名替换。例如 `import "./service.js"` 可以定位源码
`service.ts`，并非只能找字面上的 `.js`：

| 预期运行时文件 | TypeScript 查找顺序 |
| --- | --- |
| `x.js` | `x.ts` → `x.tsx` → `x.d.ts` → `x.js` → `x.jsx` |
| `x.mjs` | `x.mts` → `x.d.mts` → `x.mjs` |
| `x.cjs` | `x.cts` → `x.d.cts` → `x.cjs` |

[扩展名替换](https://www.typescriptlang.org/docs/handbook/modules/reference.html#file-extension-substitution)

`paths` 只帮助 TypeScript 理解导入，不改写发出的路径，也不会教会 Node 运行时
识别 `@/` 别名。Narrant 可以按项目映射定位源码，但不能以“图解析成功”保证运行时
可执行。当前 Node 原生 type stripping 也不读 tsconfig、不做类型检查；它的运行能力
不能代替 TypeScript checker，也不是分析 `.tsx` 的后端。
[paths](https://www.typescriptlang.org/tsconfig/paths.html)、
[Node TypeScript 支持](https://nodejs.org/api/typescript.html)

JS 输入对 `require()`、`exports.x`、`module.exports` 有专门的导入/导出识别。
TS 输入还涉及 `import = require()`、`export =` 等语言形式，以及模块输出与
`verbatimModuleSyntax` 规则；把输出设为 CommonJS，不等于切换到 JS 的推断模式。
JS 已纳入首版范围，需要独立覆盖这些源码形式，不能只增加文件后缀。
[JavaScript CommonJS 支持](https://www.typescriptlang.org/docs/handbook/type-checking-javascript-files.html#commonjs-modules-are-supported)、
[verbatimModuleSyntax](https://www.typescriptlang.org/tsconfig/verbatimModuleSyntax.html)

TS7 改动的重点是旧 JS/JSDoc 特殊推断，例如构造函数加 prototype、`this` 别名、
部分 `@enum/@class` 和 Closure 风格类型。v7.0.2 的兼容清单还列出 `module.exports`
别名、单属性 `require("x").p`、混合整对象与属性赋值等差异。这些是旧 Node JS 仓库的
实际兼容样本；“现代 CommonJS 支持”不能替代对这些旧写法的验证。
[v7.0.2 JavaScript 与 CommonJS 差异](https://github.com/microsoft/TypeScript/blob/v7.0.2/tsc/CHANGES.md)

workspace 与 project references 解决不同问题。包管理器 workspace 通常建立包名到
本仓库子包的符号链接；references 则建立不同 tsconfig 项目的关系及声明输入。
不能把所有裸包导入都当第三方，也不能把 `node_modules` 路径都一刀切成外部源码。
应同时检查受控快照中的仓库归属与真实路径，保持逻辑包名、条件入口和项目配置。
[npm workspaces](https://docs.npmjs.com/cli/v11/using-npm/workspaces/)、
[Project References](https://www.typescriptlang.org/docs/handbook/project-references.html)

**运行时类型范围已接受，版本待固定**：ECMAScript 标准声明为数组、Promise、Map 等提供
类型关系；DOM 声明按目标环境选择。Node 的 `process`、Buffer、内建模块等类型通常
来自 `@types/node`，不是 ECMAScript lib 自带内容。允许该包也应界定传递声明闭包，
维护者当前 package.json 例如声明了 `undici-types` 依赖。
[lib 选项](https://www.typescriptlang.org/tsconfig/lib.html)、
[@types/node 元数据](https://github.com/DefinitelyTyped/DefinitelyTyped/blob/master/types/node/package.json)

对运行时类型应固定环境档与版本并允许必要的传递类型，避免只放行
`@types/node` 顶层目录后又截断其内部依赖。npm 的安装位置不能单独决定它属于运行时
还是第三方业务依赖。若要求使用随附标准库，还应明确是否禁用 `@typescript/lib-*`
替换查找；`libReplacement` 是另一条可把 npm 声明带入标准库的路径。
[标准库替换](https://www.typescriptlang.org/tsconfig/libReplacement.html)

静态读取已存在的 package/config JSON 不需要执行 npm、Node 项目入口、构建脚本或
动态配置。缺失的第三方 `extends` 配置同样不能假定不存在影响：要么按获准的配置
元数据策略读取，要么把配置不完整作为分析局限。禁依赖策略不得通过安装包来暗中补齐。

### 随附运行时与执行边界

正常 compiler 不执行所分析程序；Language Service 插件则能在 tsserver 进程中运行，
并可以更改定义查询结果。裸 Compiler API 模式不需要加载仓库里的插件或执行被分析模块。
[Compiler 安全属性](https://github.com/microsoft/TypeScript/wiki/tsc-Security-Properties)、
[Language Service Plugin](https://github.com/microsoft/TypeScript/wiki/Writing-a-Language-Service-Plugin)

这不构成文件系统沙箱。建议由宿主定义 repository、标准库和已授权依赖的可读边界，
禁止 emit，固定 compiler 加载路径；不要从被分析仓库 require TypeScript、配置代码或
Yarn PnP hook。若要支持这些扩展，应单独定义受信任的能力。

Tauri 能按平台随附外部二进制。Node worker 可以采用“Node + 固定 JS bundle + 标准库资源”
或 Node SEA；SEA 的模块加载、资产和平台签名需要专门适配，不是把脚本改个后缀。
本次没有验证安装包尺寸、签名、公证或 Windows 取消行为。
[Tauri sidecar](https://v2.tauri.app/develop/sidecar/)、
[Node SEA](https://nodejs.org/api/single-executable-applications.html)

进程隔离使 Rust 可以在超时后终止 worker；合作式 cancellation 仍需要 compiler
检查 token，不能依赖事件循环正在忙于同步类型查询时立即处理普通 stdin 消息。
启动时使用可执行路径和参数数组，避免 shell，并设置输出上限、请求期限和强制回收。
这些属于候选协议的实现要求，尚未实测。
[Node 子进程接口](https://nodejs.org/api/child_process.html)

## tsserver 与 typescript-language-server

tsserver 是 TypeScript 6 及以前的独立语言服务，使用自身协议。其项目系统区分
configured、inferred、external project，一个文件可能属于多个项目。
`typescript-language-server` 是社区维护的 LSP 包装，不是 Microsoft 的原生 TS7 服务；
它的当前 README 明确安装 `typescript@6`。
[tsserver](https://github.com/microsoft/TypeScript/wiki/Standalone-Server-%28tsserver%29)、
[typescript-language-server](https://github.com/typescript-language-server/typescript-language-server)

包装层可指定 tsserver 路径、禁用自动类型获取、配置插件；插件通过 Node require 加载。
选它能复用现有 LSP 传输，但仍然保留 Node 和 TS6 的生命周期，且要防止自动选择仓库内
compiler/插件。直接使用 tsserver 则需要适配非 LSP 协议及其取消机制。
[配置文档](https://github.com/typescript-language-server/typescript-language-server/blob/master/docs/configuration.md)

TLS **v6.0.0** 的官方接口包含 `typescript.tsserverRequest`，能通过 LSP
`workspace/executeCommand` 向 semantic tsserver 透传命令。TS5.9.3/TS6.0.3 的
`openExternalProject` 可传 `rootFiles`、compiler options 和 `typeAcquisition`。
这为“完整仓库文件清单 + noResolve + 明确标准库”的项目输入提供入口，不要求把整个
分析器改写成 Compiler API。它是具体服务器扩展，不是所有 LSP 服务都支持的通用能力。
[TLS v6 README](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/README.md)、
[透传实现](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/commands/tsserverRequests.ts#L20-L51)、
[TS6 外部项目协议](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/protocol.ts#L1388-L1407)。

本地 TS5.9.3 进程探针中，完整仓库 roots 加 noResolve 保留 TS 跨 barrel/Promise 与
JS CommonJS 的本地定义目标，已安装样例第三方声明不再进入项目；漏列本地 roots 则
本地目标也消失。这只验证 tsserver 私有协议和一个小项目。TLS 的 didOpen 和查询
是否始终选中受控 external project，仍需实测；不得把配置文件本身直接列为外部 roots
然后无意建立使用原始依赖策略的 configured project。
[探针记录](typescript-code-graph-plan.md#tsserver-项目范围探针)、
[项目创建源码](https://github.com/microsoft/TypeScript/blob/v5.9.3/src/server/editorServices.ts#L5336-L5402)。

TLS v6.0.0 要求 Node >=22.22.2。采用传统 LSP 路线仍有 Node 运行时交付工作；
是否使用 LSP 与是否需要 Node 是两个问题。固定版本需连同 tsserver、标准库和 Oxc 一起验收。
[v6 发布说明](https://github.com/typescript-language-server/typescript-language-server/releases/tag/v6.0.0)。

## Oxc 的边界

Oxc parser 提供 Rust AST；SourceType 支持 TS、TSX、JS/JSX、MTS/CTS 和声明文件模式。
固定到 0.150.0 时，`from_path` 对 `.js` 选择 JavaScript Standard，对 `.jsx` 才启用
JSX；`.ts` 与 `.tsx` 同样区分。它对模块/脚本仅按扩展名作部分推断，不能代替
TypeScript 的 package.json + compilerOptions 语义。实际 `.d.mts/.d.cts` 及 script 模式
应固定版本后用 fixture 验证。更细比较见 [parser 调研](typescript-parser-comparison.md)。
[Oxc parser](https://oxc.rs/docs/guide/usage/parser.html)、
[固定版本 SourceType](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_span/src/source_type.rs)

`oxc_semantic` 提供单个 AST 的作用域、绑定、引用和可选 CFG。SymbolId、ReferenceId
用于本次分析中的实体访问；这些结构没有 TypeScript Program/TypeChecker 的跨项目类型
推断契约。即使能识别 type-position 引用，也不能据此推导 `object.method()` 的实例类型。
[oxc_semantic](https://github.com/oxc-project/oxc/blob/main/crates/oxc_semantic/src/lib.rs)

`oxc_resolver` 负责 ESM/CommonJS 模块文件解析，支持 tsconfig extends、paths、references
和自定义 FileSystem。默认扩展名与条件需要按用途配置；它解析的是文件位置，不是
TypeScript 类型或方法实现。单靠 resolver 的结果不能把导入标识符变成最终 callable。
[Oxc Resolver](https://github.com/oxc-project/oxc-resolver/blob/main/README.md)

Oxc 当前的 type-aware linting 由独立 Go 组件 tsgolint 建立 TypeScript program 并运行
规则，Rust Oxlint 负责其他流程。这个分工直接说明，不能把 Oxc 的局部 semantic
能力宣传成完整 TypeScript checker。要复用 tsgolint 的 checker 还会引入另一条原生
内部接口依赖，并非给 `oxc_semantic` 打开一个类型检查开关。
[Oxc type-aware 架构](https://oxc.rs/docs/guide/usage/linter/type-aware.html#overview)

**建议草案**：如果选 TS7 LSP，先只让 Oxc 负责语法、own content、源位置和局部绑定；
跨文件目标由 TypeScript 提供证据。除非明确需要独立降级模式，不要同时引入第二套
resolver 决定相同引用的目标，以免同一调用出现两种不一致的解析结果。

## SWC 与 Tree-sitter

SWC 有 Rust TS/JS parser 和代码生成工具。其 resolver 为词法作用域中的标识符赋予
SyntaxContext；这与基于 TypeScript 类型确定方法目标不同。官方迁移指南明确 SWC
逐文件转译，不做 TypeScript 类型检查。SWC 可替换语法层，不能独立替换 checker。
[SWC parser](https://rustdoc.swc.rs/swc_ecma_parser/)、
[SWC resolver](https://rustdoc.swc.rs/swc_core/ecma/transforms/base/fn.resolver.html)、
[从 tsc 迁移](https://swc.rs/docs/migrating-from-tsc)

Tree-sitter 构建可增量更新、容错的 concrete syntax tree；TypeScript 与 TSX 使用
不同 grammar，JavaScript/JSX 使用对应 JS grammar。标准 Rust Node 接口提供 byte range
及 error/missing 状态。它是可嵌入 parser，没有内建 tsconfig 项目模型或 TypeScript checker。
[Tree-sitter 简介](https://tree-sitter.github.io/tree-sitter/)、
[TypeScript grammars](https://github.com/tree-sitter/tree-sitter-typescript)、
[Rust Node 接口](https://docs.rs/tree-sitter/latest/tree_sitter/struct.Node.html)

**推论**：Tree-sitter 的语法错误恢复对编辑中代码有用，但 Narrant 当前处理固定快照。
若 selected file 出现不受支持语法或 parse error，应按约定显式失败或标记能力缺失，
不能因为 parser 返回了一部分树就当作已完整提取。其增量优势也不能直接换算成本任务提速。

## 文件范围、声明与源码规范化

| 形式 | 与后端有关的事实 | 图规则与验证边界 |
| --- | --- | --- |
| `.ts/.tsx` | TypeScript 支持 JSX 的类型检查；intrinsic 标签与值组件走不同检查规则 | 已确认仓库内组件使用关系；HTML 标签不生成对象或关系，源码内容保留 |
| `.mts/.cts` | Node 模式下分别强制 ESM/CommonJS；影响 resolution conditions | 支持这些扩展名时不能只改文件过滤器 |
| `.d.ts/.d.mts/.d.cts` | 声明可提供符号和签名，但并不提供业务实现体 | 显式选中的声明文件如何入图，依赖声明如何保持 External |
| `.js/.jsx/.mjs/.cjs` | allowJs 控制 JS 进入 Program，checkJs 控制 JS 错误报告；两者不同 | 已纳入首版；测试按明确文件约定识别，不承诺任意动态目标完整可解 |
| JSDoc 与 triple-slash 指令 | 部分注释会影响类型或文件上下文 | JSDoc 复用已有能力，不增加特殊适配；语义服务保留原文，项目指令仍由后端解释 |

上述语义来自
[模块格式与解析](https://www.typescriptlang.org/docs/handbook/modules/reference.html)、
[JSX](https://www.typescriptlang.org/docs/handbook/jsx.html)、
[allowJs](https://www.typescriptlang.org/tsconfig/allowJs.html)、
[checkJs](https://www.typescriptlang.org/tsconfig/checkJs.html)、
[JSDoc 类型](https://www.typescriptlang.org/docs/handbook/jsdoc-supported-types.html)、
[Triple-Slash Directives](https://www.typescriptlang.org/docs/handbook/triple-slash-directives.html)。

源码范围只用于适配器内部定位，不能作稳定身份。Oxc Span 截取 Rust `str` 的 byte range，
而 TypeScript LSP 需要按协商编码转换位置。中文、emoji、CRLF、BOM、同名重载必须有 fixture。
[Oxc Span 实现](https://github.com/oxc-project/oxc/blob/main/crates/oxc_span/src/span.rs)

**建议草案**：ID 从仓库相对文件身份、词法所有者、native kind 和声明名字产生，重复
匿名/同名对象采用明确消歧约定；不可复用 Oxc SymbolId、TypeScript 内部 symbol id、AST
地址或 LSP 行号。跨文件合并声明已确认分别显示；同 owner overload 按一个 Callable
保留全部签名和实现进行验证。直接 const 绑定的箭头与参数回调按语法位置区分，
符合最小规则的具名箭头独立建模。保留签名与 own code 时，仍需区分
独立子对象与内联回调，避免子方法变化递归修改整个父类。

## 建议的验证出口

用户明确偏向 **LSP + Oxc**，默认仓库内 + 标准库；JSDoc 已有能力保留，不增加特殊适配。
先验证固定 TLS + 传统 tsserver 的项目输入、TS5/6 兼容和生命周期；TS7 原生 LSP
作为另一服务器候选。Compiler API 保留为缺少必要证据或项目控制入口时的备选，
不能把它提前写成最后选择。VS Code 当前稳定版仍默认使用传统 tsserver 的事实支持
继续评估这条路线，但不能代替 Narrant 自己的默认依赖范围与图映射验收。

| 验证项 | 必须能够区分的情况 |
| --- | --- |
| 实际版本 | npm 7.0.2、7.1 unstable 接口、固定 TS5.9/TS6 API；不能把版本系列兼容目标当作单一 checker 等价承诺 |
| 项目上下文 | 单 tsconfig、多 references、shared file、多 package、paths、exports conditions、旧 baseUrl |
| 符号证据 | alias/reexport、命名箭头、overloads、interface merging、继承、JSX、动态 callback、computed access |
| 不完整环境 | 缺 node_modules、缺 extends config、缺 project `.d.ts`、parse error、server failure；不得都变成空图 |
| 快照上下文 | 不同绝对目录得相同 ID；两侧依赖策略一致；第三方源码/声明不进入默认项目，metadata 与 runtime 类型单独记录；无 repository code 执行和依赖安装 |
| 规范化 | 纯格式无差异，字面量/ASI/JSX 文本变动不丢失，子对象归属正确；不提取 JSDoc 契约，语义服务保留原文 |
| 生命周期 | 初始化逆向请求、语义请求取消、超时、正常 shutdown、协议错误、退出与子进程清理 |

用户已确认缺依赖仍出图，必须保留“看到了引用但无法证明目标”与“没有引用”的区别。
是否以及如何在 Structure Changes 中展示分析局限属于产品决定，本篇不提前改变现有 UI
或公共 Code Graph 契约。

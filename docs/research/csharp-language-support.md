# C# 语言支持调研

后续工程草案：[公共进程协议](../specs/0024-native-analyzer-helper-protocol.md)、[语言配置与验收](../specs/0024a-native-language-analysis-profiles.md)。本文保留调研过程中的候选和历史待决项；已确认决策以 ADR-0034 为准，工程建议以草案和后续实测修正为准，未验收项不构成发布承诺。

共同输出语义已由 [ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 确认：helper 返回单张配置并集图，对象和已解析关系至少在一种已分析配置下成立，不保留配置归属，不新增配置切换/变体界面。必要的配置分析与兼容源码对象归并在 helper 内完成；无法正确归并按完整图契约失败。下文提到的配置结果封套、条件图或变体展示仅是未采用的研究候选，具体发现算法与后端能力仍须验证。

调研日期：2026-10-02。仓库基线：`6761aca6907a3fd2f1ca730fa6a180656f132e7f`。

## 后续已决范围

[ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 已确定 Java、Go、C#、C/C++ 的接入边界：各自原生 helper 在同一进程中取得语法与语义，负责语言类别、内容投影、稳定身份、图成员及 Node / External / Unresolved 关系分类，通过统一接口返回规范化 `CodeGraph` 构造 DTO。Rust 负责输入来源边界、协议、通用校验、最终 `CodeGraph` 构造与生命周期；不跨进程传输编译器整棵 AST 再由 Rust 完成语言映射。

四门新语言均禁止执行被分析仓库及依赖携带的构建脚本、插件、注解处理器、生成器等代码；可以使用受控分析工具，但不能借项目加载触发上述执行。C# 的 MSBuild 导入因此排除在当前及默认未来路线之外，下面保留其事实与实验设计作为已排除候选的研究证据。

默认分析范围为仓库源码与明确目标的标准库/平台声明；不下载或 restore 第三方依赖，不自动纳入本机第三方缓存或环境，也不因业务依赖已经下载而允许读入。已提交的源码按项目与选集规则处理，不因位于 `vendor/` 一概排除。Rust、TS/JS、Python 现状不变，Rust 可选 `all-dependencies` 保留。字段级协议、目标支持矩阵及具体映射仍待设计和验收，本文不表示这些能力已经实现或验证。

本文以 **事实** 标记一手文档或源码能够直接支持的结论，以 **推断** 标记由事实得出的工程判断，以 **建议** 标记 Narrant 的候选实现政策，以 **未验证** 标记仍需运行探针的事项。`main` 源码链接表示调研时观察到的上游实现，不承诺其行为与任意旧版 NuGet 完全相同；正式支持包必须锁定版本、源提交和散列后重复验收。

## 当前仓库给出的边界

**事实（本地代码）：** 本轮实际阅读了 `AGENTS.md`、`CONTEXT.md`、领域说明、0018/0018b、0019、0021、0022、0023 及 ADR-0030–0033，并继续阅读下列实现；结论不以旧调研替代当前代码。

**诊断边界：** 本文的诊断封套指 helper → Rust adapter 的私有证据 DTO 或日志，不是现有 `CodeGraph` / `StructureComparison` 的字段；`Unresolved` 仍只保存观察名称。若要向用户展示缺失上下文、降级原因或配置提示，需要另行设计 application DTO、传输与 UI，本方案尚未实现该扩展。无法满足所选分析模式时沿现有错误通道失败，不能把内部诊断封套当作成功返回部分图的依据。[分析接口](../../src-tauri/src/structure_review/code_graph/analyzer.rs)、[图目标](../../src-tauri/src/structure_review/code_graph/node.rs)、[应用结果](../../src-tauri/src/structure_review/application/projection.rs)

| 代码证据 | 对 C# 接入的约束 |
| --- | --- |
| [LanguageAnalyzer / AnalysisInput](../../src-tauri/src/structure_review/code_graph/analyzer.rs) | 输入为固定侧 root、存在于该侧的相对路径和 `include_tests`；选集决定图成员，不限制读取同侧上下文。路径越界、选中文件读取/提取失败是错误 |
| [node.rs](../../src-tauri/src/structure_review/code_graph/node.rs)、[graph.rs](../../src-tauri/src/structure_review/code_graph/graph.rs)、[validation.rs](../../src-tauri/src/structure_review/code_graph/validation.rs) | 六类节点、单个声明归属、虚拟根可达；Node 必须在图中。External 有 Repository/Dependency 描述；Unresolved 只有观察名，不是“未知外部符号” |
| [diff.rs](../../src-tauri/src/structure_review/code_graph/diff.rs) | signature/code/extensions 参与自身比较，source 不参与；contains/references 按集合比较，不能借数组顺序记录初始化先后 |
| [Rust 入口](../../src-tauri/src/structure_review/rust_analyzer/mod.rs)、[syntax](../../src-tauri/src/structure_review/rust_analyzer/syntax.rs)、[references](../../src-tauri/src/structure_review/rust_analyzer/references.rs)、[symbols](../../src-tauri/src/structure_review/rust_analyzer/symbols.rs) | 本地语法库 + rust-analyzer；依赖范围是 Rust 策略，稳定 ID 与目标类别验证属于适配器，不由通用图猜测 |
| [TS 入口](../../src-tauri/src/structure_review/typescript_analyzer/mod.rs)、[identity](../../src-tauri/src/structure_review/typescript_analyzer/identity.rs)、[project](../../src-tauri/src/structure_review/typescript_analyzer/project.rs)、[references](../../src-tauri/src/structure_review/typescript_analyzer/references.rs)、[targets](../../src-tauri/src/structure_review/typescript_analyzer/reference/targets.rs) | Oxc 与受控 tsserver 项目分工；逐项目比较证据，冲突为 Unresolved；上下文读取不扩大图成员，声明位置和合格种类都要核对 |
| [Python 入口](../../src-tauri/src/structure_review/python_analyzer/mod.rs)、[syntax](../../src-tauri/src/structure_review/python_analyzer/syntax.rs)、[references](../../src-tauri/src/structure_review/python_analyzer/references.rs)、[session](../../src-tauri/src/structure_review/python_analyzer/session.rs) | Ruff + ty；当前默认只读仓库及固定标准库，并有专用环境、别名、动态绑定规则。这是 Python 已确认范围，不能直接复制成 C# 产品决定 |
| [两侧应用执行](../../src-tauri/src/structure_review/application/mod.rs)、[before 生命周期](../../src-tauri/src/structure_review/application/workspace.rs) | 包先准备，真实 worker 再持有工作区和包租约；复用 Review head，仅额外一个 before；两侧并行，任一硬错误取消同组，join 和 cleanup 后才返回 |
| [语言包服务](../../src-tauri/src/language_support/mod.rs)、[model](../../src-tauri/src/language_support/model.rs)、[storage](../../src-tauri/src/language_support/storage.rs)、[精确版本表](../../resources/language-support-packs.json)、[构建脚本](../../scripts/build-language-support-packs.mjs) | 当前仅 Rust、TS/JS、Python 三包；固定版本、摘要、必需入口、租约保护和原生 smoke 均需扩展，添加语言名不是完成接入 |

**建议：** C# 沿用这些公共边界。两侧使用同一 helper/解析版本和政策；各侧项目配置可以随提交变化，但不能把宿主环境的随机变化当代码差异。不新增语言专属 checkout，不把清洗后的 `.cs`、`.csproj` 或声明库复制到影子源码目录。可信 helper/control 文件可放临时目录；项目配置归一化和 override 保留内存。支持包内固定参考程序集从安装目录读取。

## 推荐路线与候选后端

**建议：使用 Narrant 自有、版本固定、独立进程运行的 Roslyn helper 落实已决架构。** 同一套 Roslyn `SyntaxTree` 负责声明和自身内容，`SemanticModel`/`IOperation` 负责可证实的语义关系；helper 完成语言映射并返回规范化构图 DTO，不导入 MSBuild。理由是 Narrant 需要语义符号、精确声明位置、多声明合并和源码内容归属；编译器 API 可以直接提供这些事实。`CSharpCompilation.Create` 可直接接收语法树、引用和编译选项，`GetSemanticModel` 可取得对应树的语义模型。[Create API](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.csharpcompilation.create?view=roslyn-dotnet-5.3.0)、[GetSemanticModel API](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.csharpcompilation.getsemanticmodel?view=roslyn-dotnet-5.3.0)

下表保留后端比较证据；LSP 与 Rust 内语法抽取是原调研候选，不取代已决的同进程原生 helper 路线。

**事实：稳定 compiler API 与独立 LSP 是不同发布线。** `Microsoft.CodeAnalysis.CSharp` 的稳定版本 `5.9.0` 已于 2026-08-17 发布，标记 MIT，包包含 `net10.0` 与 `netstandard2.0` 资产，并指向源提交 `35d9211b841e7613c1d2f8f5af6d628ace696c4c`。可采用它作为新版本候选；为使 `net8.0` 最小探针更保守，也已核实稳定 `5.3.0` 存在，不能把预发布 LSP 的版本号直接当作稳定 API 版本。[5.9.0 包](https://www.nuget.org/packages/Microsoft.CodeAnalysis.CSharp/5.9.0)、[5.3.0 包](https://www.nuget.org/packages/Microsoft.CodeAnalysis.CSharp/5.3.0)

| 候选 | 当前已核实事实 | 选型判断 |
| --- | --- | --- |
| Roslyn 原生 helper | 编译器平台为 MIT；官方 FAQ 明确建议通过 Roslyn NuGet 包再分发 DLL。[许可](https://github.com/dotnet/roslyn/blob/main/License.txt)、[FAQ](https://github.com/dotnet/roslyn/blob/main/docs/wiki/FAQ.md) | 首选。helper 内使用符号/operation/声明元数据完成语言映射，输出规范化构图 DTO；代价是维护 C# helper、协议和静态配置策略 |
| 独立 Roslyn LSP | 官方 Microsoft/RoslynTeam NuGet `roslyn-language-server`，本次所见 `5.12.0-1.26426.8`，MIT、预发布、要求 .NET 10 runtime，支持 stdio/pipe。维护者明确其 supported 但 experimental，CLI/发布政策可能变动。[固定版包](https://www.nuget.org/packages/roslyn-language-server/5.12.0-1.26426.8)、[维护者答复](https://github.com/dotnet/roslyn/discussions/82317) | 第一备选，应与 helper 做同 fixture 实测。可以复用 LSP 客户端，但标准导航结果不足以直接给出 Narrant 所需全部语义分类；不能按旧资料以“只在 VS Code 中可用”排除它 |
| OmniSharp | 仓库 MIT，提供 HTTP/stdio 服务；`v2.0.0` 已移除 .NET Framework/Mono 相关实现并支持 `.slnx`。1.40 已转向 Roslyn BuildHost。[项目](https://github.com/OmniSharp/omnisharp-roslyn)、[许可](https://github.com/OmniSharp/omnisharp-roslyn/blob/master/license.md)、[v2.0.0](https://github.com/OmniSharp/omnisharp-roslyn/releases/tag/v2.0.0)、[changelog](https://github.com/OmniSharp/omnisharp-roslyn/blob/master/CHANGELOG.md) | 历史成熟但不是本次首选；不得沿用“最新版有 Mono 所以 legacy 支持最强”的旧判断。未核验 v2 实际 runtime 与各平台 payload；也不能把其默认自有协议等同完整标准 LSP |
| csharp-ls | `0.28.0` 包为 MIT、`net10.0`，文档要求 .NET 10 SDK，支持更早目标框架；有标准 LSP 及 `csharp/metadata` 扩展，依赖 Roslyn/MSBuildWorkspace/Locator/ILSpy。[固定版包](https://www.nuget.org/packages/csharp-ls/0.28.0)、[项目文件](https://github.com/razzmatazz/csharp-language-server/blob/master/src/CSharpLanguageServer/CSharpLanguageServer.fsproj) | 第二 LSP 备选；注意 SDK 安装要求与目标 TFM 分开。社区治理、依赖闭包和自定义 generated/decompiled URI 增加集成验证面 |
| tree-sitter-c-sharp | MIT；Rust binding 提供 `LANGUAGE`、`NODE_TYPES`。README 声称 C# 1–14，并列出上下文关键字/文件式应用 `#:` 指令例外。[项目](https://github.com/tree-sitter/tree-sitter-c-sharp)、[许可](https://github.com/tree-sitter/tree-sitter-c-sharp/blob/master/LICENSE)、[Rust binding](https://github.com/tree-sitter/tree-sitter-c-sharp/blob/master/bindings/rust/lib.rs) | 可作为 Rust 内声明抽取备选；它不是类型/重载/跨项目语义引擎。若同时引入 Roslyn，就多维护一套位置与语法映射，首期收益不明确 |

### 开源 Roslyn 与微软扩展发行物必须区分

**事实：** `dotnet/vscode-csharp` 源码采用 MIT，但其扩展发行物另有 runtime license，限定与 VS Code、Visual Studio、Xamarin Studio 等规定产品配合使用，并限制独立提供/分享。C# Dev Kit 官方 FAQ 明确它不是开源产品，其个人/开源/组织订阅使用资格不等于第三方再分发许可。[C# 扩展 README](https://github.com/dotnet/vscode-csharp)、[发行许可](https://github.com/dotnet/vscode-csharp/blob/main/RuntimeLicenses/license.txt)、[Dev Kit FAQ](https://code.visualstudio.com/docs/csharp/cs-dev-kit-faq#_licensing-and-contributing)、[订阅说明](https://learn.microsoft.com/visualstudio/subscriptions/vs-c-sharp-dev-kit)

**建议：** 不从 VSIX 中抽取 `Microsoft.CodeAnalysis.LanguageServer` 或 Dev Kit 扩展 DLL 作为支持包。选择 Roslyn 官方 MIT NuGet 依赖、自建 helper，或明确 MIT 的独立 `roslyn-language-server` 包；同名服务器程序集并不能证明取得方式和许可相同。**未验证：** 独立 LSP 实际下载包的完整 payload、所有传递依赖/附带扩展的许可证闭包尚未检查；正式分发仍需针对锁定产物审计。Dev Kit 的 [Marketplace EULA 页面](https://marketplace.visualstudio.com/items/ms-dotnettools.csdevkit/license) 本轮仅取得页面外壳，本文不声称逐条审读其完整 EULA。

**事实：** 独立 Roslyn LSP 当前发布配置包含 Windows、Linux、Linux-musl、macOS 的 x64/arm64 RID，但配置为 `SelfContained=false`；apphost 不等于附带 runtime。[发布配置](https://github.com/dotnet/roslyn/blob/main/src/LanguageServer/LanguageServerPublish.props) **建议：** 如选择 LSP，支持包需另附其明确要求的固定 runtime，并确认实际版本产物与源码配置一致。

## 三类不能混为一谈的 .NET 依赖

| 依赖 | 作用 | 固定支持包策略 |
| --- | --- | --- |
| helper / LSP 所需 .NET runtime | 执行 Narrant 选择的工具程序 | 建议按 RID self-contained 发布或在包内放固定私有 runtime；不得要求用户在 PATH 中碰巧安装同版本 |
| 被分析项目的 target framework 和 reference assemblies | 决定项目能够引用哪些 API；例如 `net8.0`、`net10.0`、`net48` | 必须来自目标 TFM 对应 targeting pack 或经验证的 reference package；不能用 helper 自己的运行时程序集冒充 |
| 项目 SDK / MSBuild / workloads | 解释项目导入、条件、SDK tasks、资源与平台目标 | 固定 runtime 不能替代它；但 MSBuild 导入已排除，不能为取得项目配置而启动 SDK tasks/workloads。仅静态读取可证明的配置与明确目标平台声明 |

**事实：** self-contained 发布包含应用所需 runtime，framework-dependent 发布仍要求宿主安装兼容 runtime；`global.json` 选择的是 SDK，其选择与项目目标 runtime 独立。SDK resolver 还会向项目/solution 的父目录搜索 `global.json`，因此固定工作区路径和父目录环境也是可重复性的输入。[发布模式](https://learn.microsoft.com/en-us/dotnet/core/deploying/)、[global.json](https://learn.microsoft.com/en-us/dotnet/core/tools/global-json)

**事实：** reference assembly 提供编译需要的 API 元数据而非实现；Microsoft 推荐优先用 reference assemblies，手工调用 Roslyn 引用 API 时必须选取正确目标平台版本。`.NET Framework` 使用 targeting packs；现代 .NET 的核心参考包为 `Microsoft.NETCore.App.Ref`。**建议：** 读取 metadata 不应通过 `Assembly.Load` 把项目依赖当作可执行程序集加载；依赖缺失时保留声明与诊断，不能借用宿主 `System.Private.CoreLib` 声称目标框架已经完整恢复。[参考程序集](https://learn.microsoft.com/en-us/dotnet/standard/assembly/reference-assemblies)

## 项目加载、配置与缺失上下文

### MSBuildWorkspace 的能力和边界（已排除候选）

以下保留候选研究事实；不构成当前或默认未来实现步骤。设计时加载存在执行项目代码的能力，因此不能通过用户已安装 SDK 或 Workspace Trust 自动启用。

**事实：** `MSBuildWorkspace` 可以打开 solution 或 project；`OpenProjectAsync` 会加载引用项目，`Create(properties)` 接收与 MSBuild `/property` 对应的全局属性，提供 `Diagnostics`/`WorkspaceFailed`。`LoadMetadataForReferencedProjects=true` 优先使用现有输出程序集；找不到才打开项目。因此“存在旧 DLL”与“本侧源码正确”不是同一个保证。[MSBuildWorkspace 源码](https://github.com/dotnet/roslyn/blob/main/src/Workspaces/MSBuild/Core/MSBuild/MSBuildWorkspace.cs)

**原候选建议（已排除）：** MSBuild 路线曾建议保持 `LoadMetadataForReferencedProjects=false`，避免旧 `bin` 输出代替本侧源码。**仍适用的建议：** 以 repository-relative project path + `Configuration` + `Platform` + TFM 作为语义上下文身份；静态配置对 `.sln`、`.slnx`、单 `.csproj`、无项目 `.cs` 的覆盖分别验收，多个 solution 重叠、linked source 被多个项目纳入、同文件多 TFM 不得静默“取第一个”。

**事实：** 当前 Roslyn build host 对 SDK-style 项目优先选择 .NET host；传统项目在 Windows 尝试 .NET Framework host，在其他平台尝试 Mono，找不到可用 MSBuild 时才 fallback 到 .NET SDK 并记录可能出错的警告。该 fallback 不能证明 legacy 项目支持成功。[BuildHostProcessManager 源码](https://github.com/dotnet/roslyn/blob/main/src/Workspaces/MSBuild/Core/MSBuild/BuildHostProcessManager.cs)

**推断：** SDK-style 与 legacy `.csproj`，尤其旧 ASP.NET、COM、WPF/XAML、Windows-specific tasks、第三方 SDK/workloads，不能仅按项目标签承诺支持。已决路线须另验静态配置可覆盖的范围；拥有 .NET runtime 不等于拥有对应 targeting pack，更不代表允许执行项目导入。

### 多 TFM、条件编译和配置

**事实：** 当前 `ProjectBuildManager` 在只有 `TargetFrameworks` 时逐个设置 `TargetFramework` 并执行设计时构建；若已经指定单个 `TargetFramework` 则只构建该上下文。解析选项同时携带语言版本、preprocessor symbols 和 regular/script code kind。[多 TFM 设计时构建源码](https://github.com/dotnet/roslyn/blob/main/src/Workspaces/MSBuild/BuildHost/Build/ProjectBuildManager.cs)、[CSharpParseOptions](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.csharpparseoptions?view=roslyn-dotnet-5.3.0)

**推断：** 同一个 `.cs` 文件在不同 TFM/配置下可能有不同声明和调用目标，因而“所有上下文的关系取并集并标成确定”会失真。**建议：** 首期固定一个明确可见的分析上下文，或在后续图契约中加入上下文维度；无论选哪条路，都把上下文写入缓存键和诊断。未选择配置时不能根据列表顺序隐式建立跨次运行不稳定的身份。

### Restore 不等于纯下载

**事实：** NuGet restore 读取项目间引用、项目属性和目标框架，下载包并写 `project.assets.json`、`.nuget.g.props` 和 `.nuget.g.targets`；包本身可以带入 `build`、`buildMultiTargeting`、`buildTransitive` 逻辑。传统 `packages.config` 的 MSBuild restore 与现代 `PackageReference` 还存在不同限制，不能统一当成 `dotnet restore`。[NuGet restore targets](https://learn.microsoft.com/en-us/nuget/reference/msbuild-targets)、[包内 props/targets](https://learn.microsoft.com/en-us/nuget/concepts/msbuild-props-and-targets)

**后续已决：** 不为被分析项目下载或 restore 第三方依赖，也不复用本机 NuGet 第三方缓存作为默认上下文；已有 assets 不能将其中的业务依赖自动加入允许范围。只读取仓库源码及明确目标的标准库/平台 reference profile，缺少所需声明或配置时按支持范围报告上下文不足。Workspace Trust 不放宽此边界。

**未验证（原候选研究）：** 已有 NuGet 全局缓存但固定工作区没有 `obj/project.assets.json` 时的恢复能力、重定向 intermediate paths 对自定义 targets 的影响，以及跨平台项目 SDK/TFM/RID/workload 的覆盖率均未实测。前两项不再是当前路线的交付前提；目标 reference profile 与静态配置覆盖仍须验收。

## 执行代码的边界

**事实：** Microsoft 明确把 build、restore，甚至在 IDE 中打开项目都视为需要信任源码的行为；项目/导入 targets 可执行任意代码，父目录 `Directory.Build.*`、用户扩展及 NuGet 资产也会影响执行。[MSBuild 安全说明](https://learn.microsoft.com/en-us/visualstudio/msbuild/msbuild-security-best-practices?view=visualstudio)

**事实：** Roslyn 设计时加载设置 `DesignTimeBuild=true`、`BuildProjectReferences=false`、`SkipCompilerExecution=true`、`ProvideCommandLineArgs=true`，但仍执行 `Compile`/`CoreCompile` 及可用的 `DesignTimeMarkupCompilation` targets。**推断：** 这些选项避免普通编译产物，不是沙箱，也不意味着不会执行项目逻辑、写生成文件或启动子进程。[ProjectBuildManager 源码](https://github.com/dotnet/roslyn/blob/main/src/Workspaces/MSBuild/BuildHost/Build/ProjectBuildManager.cs)

**事实：** source generator 是在编译过程中运行的代码，能查看 Compilation 并向其加入源码；`GeneratorDriver.RunGeneratorsAndUpdateCompilation` 是显式执行入口。Workspace 的 `GetSourceGeneratedDocumentsAsync`/相关内部路径会在需要时运行 generators。分析器诊断则有 `CompilationWithAnalyzers.GetAnalyzerDiagnosticsAsync` 这一执行入口。[source generator 设计](https://github.com/dotnet/roslyn/blob/main/docs/features/source-generators.md)、[GeneratorDriver API](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.generatordriver.rungeneratorsandupdatecompilation?view=roslyn-dotnet-5.0.0)、[Project 源码](https://github.com/dotnet/roslyn/blob/main/src/Workspaces/Core/Portable/Workspace/Solution/Project.cs)、[分析器 API](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.diagnostics.compilationwithanalyzers.getanalyzerdiagnosticsasync?view=roslyn-dotnet-5.3.0)

**后续已决：** MSBuild 项目导入、项目及依赖提供的 source generators、diagnostics analyzers 均不进入当前及默认未来路线；不加载这些程序集，不调用其执行入口，也不得通过其他加载 API 间接触发。原候选的“移除 `AnalyzerReferences` 再取得 Compilation”不能消除 MSBuild 导入的执行能力。因生成符号缺失而无法绑定的边保留 Unresolved，原因仅进入 helper 内部诊断；仅关闭 `RunAnalyzersDuringBuild` 不足以证明禁止执行边界。

**建议：** 已跟踪的 `.g.cs`/`.Designer.cs` 是真实仓库源码，能按选定文件正常出图；仅在 `obj` 或 generator 内存中产生的源不是固定选择内的普通 Node。其是否作为独立 generated external、折叠到源声明，还是标记 unresolved，是待决产品问题；不能伪造一个 repository-relative path。源生成符号与编译器隐式生成的 record members、async state machine 还应区别处理。

## 不用 MSBuild 是否仍有语义能力

**事实：** `CSharpCompilation.Create` 不要求 `.csproj` 或 MSBuild；传入多棵语法树、`MetadataReference` 和选项即可取得编译与语义模型。它允许增量添加树和引用。**推断：** 这足以建立已输入源码之间的部分静态绑定，但不会自动重现项目选择、条件常量、隐式 global usings、TFM reference sets、NuGet graph 或 source generators。[Compilation API](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.csharpcompilation.create?view=roslyn-dotnet-5.3.0)

**建议：** 基础模式只读固定侧源码，使用明确的文件/项目分组及已知选项；不把所有 `.cs` 拼成一份“虚拟项目”后宣称项目语义完整。不足的上下文须进入 helper 内部诊断；仅当 Roslyn 真正绑定到非错误符号、目标来源可确认且当前诊断不破坏该绑定时，才能提升为确定关系。结构抽取仍可独立于语义能力返回有效声明，但无包、helper 崩溃、协议错误、不可读文件等硬错误必须返回失败。

### 在已决边界内细化的受控政策

禁止执行、依赖默认范围及 helper/Rust 分工已经确定；以下具体配置覆盖与映射规则仍是实现建议：

1. **受控基础模式先行。** 以编译器 API 加载原始源码和明确提供的 reference profile，禁止 MSBuild 导入、项目插件和 generator。无项目文件可以使用明确的 standalone profile；项目文件只有在有限静态配置足以证明源码归属/选项时才构造 Compilation。遇到任意 imports、条件表达式、未知 SDK 或不明 linked-source 归属，不默默猜测项目成员。可提取声明，但受该缺口影响的调用不能提升为确定边；若无法满足用户选择的项目语义等级，明确失败。
2. **MSBuild 导入已排除。** 静态配置不能解释的工程应明确报告不支持或缺少上下文，不以启动 `MSBuildWorkspace`、设计时 targets 或 restore 补齐。SDK-style 项目也必须满足这一边界。
3. **配置是语义输入。** 记录 Compile Include/Remove/Exclude、ProjectReference、Reference/aliases、LangVersion、DefineConstants、Nullable、AllowUnsafeBlocks、implicit/global usings、Directory.Build.props/targets、Directory.Packages.props、NuGet.config 和 global.json 的有效来源。静态读取不是重新实现整个 MSBuild；无法解释的输入要留下缺口，而非取 XML 中第一个值。语言版本要固定，不能无条件设置 `latest`。[编译选项语义](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/compiler-options/language)
4. **首期每个选中文件要求一个明确 project/configuration/platform/TFM。** 多项目允许，只要文件归属唯一；同文件多上下文需要明确选择，未选择时报可解释错误。未来可仿照 TS 对所有上下文求证，只保留一致目标、冲突为 Unresolved，但不能把所有关系合并成无条件事实。TFM 已在一侧移除时提示配置不再适用，不换一个目标继续。
5. **限制外部输入的来源而非只过滤输出 URI。** selected/source/config 的真实路径必须属于本侧；linked `.cs` 越界不得静默读入。明确目标的标准库/平台 reference assemblies 是单独声明的 metadata 来源，需记录身份和摘要；本机第三方依赖缓存不自动读入。排除父目录 global.json、Directory.Build.*、环境 SDK resolver、用户 MSBuild 扩展和自定义 NuGet feeds 的不受控发现。无法建立约定边界时拒绝该模式，不能只改 CWD 就宣称隔离成功。

**推断：** 固定 Git checkout 往往没有 Git 忽略的 `obj/project.assets.json`。即使用户主目录的 NuGet 缓存齐全，也未必有本侧可用的依赖图，且已决默认范围不允许借此纳入第三方业务依赖。“不执行项目加载/restore”不能同时被描述为“完整任意 csproj 语义”；换一个 LSP 也不会消除该限制。

## Roslyn 为图映射提供的直接证据

以下是后续映射规则的 API 根据，并非已经在本机通过的验收结果。

| 场景 | 已核实 API 事实与 Narrant 推断 |
| --- | --- |
| partial type / member | `DeclaringSyntaxReferences` 对 partial type 可返回多个声明；partial method/property/event 的定义与实现需要通过各自 `PartialDefinitionPart`/`PartialImplementationPart` 收集。推断：跨文件同一符号不能简单取第一处声明，图的单位置/单文件 Node 契约需要明确按 declaration part 映射。[API](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.isymbol.declaringsyntaxreferences?view=roslyn-dotnet-5.3.0) |
| LINQ query | `GetQueryClauseInfo` 提供查询子句的语义信息。建议用其绑定结果或 operation 提供目标，不能只按 `Select`/`Where` 拼接库函数名。[API](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.csharpextensions?view=roslyn-dotnet-5.3.0) |
| dynamic | Roslyn 将动态调用表示为 `IDynamicInvocationOperation`。图内 Unresolved 只保留观察名称；位置、表达式和原因放 helper 证据 DTO/诊断封套。不因为接收器曾有某个运行时值就静态选定方法。[API](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.operations.idynamicinvocationoperation?view=roslyn-dotnet-5.3.0) |
| generated documents | 普通 `Project.Documents` 不含 source-generated documents，后者需要单独 API。推断：helper 必须显式记录其来源，不能混入已选源码。[源码](https://github.com/dotnet/roslyn/blob/main/src/Workspaces/Core/Portable/Workspace/Solution/Project.cs) |

## 六类 Code Object 的建议映射

下面是 helper 内完成的建议初期 `.cs` 源码模型，Rust 只校验规范化 DTO 并构造最终图。`.csx`、Razor 的 `.razor/.cshtml`、Unity 静态配置和 file-based app 的声明覆盖分别验收；XAML 等生成过程及构建指令执行不在允许范围。已跟踪的普通 C# 文件不因 `.g.cs` 后缀一律丢弃。

| 类别 | C# native kind 与范围 | 自身内容和子对象 |
| --- | --- | --- |
| Container | `file`、`namespace`、`file_scoped_namespace` | 文件容器只组织选中声明；namespace 以显式声明片段为单位，不能把跨文件 namespace 合成多父节点。文件原文用于展示；using/extern alias/global attributes/预处理配置的有意义变化另保留规范化语义内容 |
| Type | `class`、`struct`、`interface`、`enum`、`record_class`、`record_struct`、`delegate`，含显式嵌套类型 | 头部包含修饰符、attribute、基类/接口、泛型参数/约束及 primary-constructor/record 参数；独立成员内容从父投影剥离。`using X = ...` 是别名，不凭语法冒充新类型 |
| Callable | `method`、`constructor`、`static_constructor`、`destructor`、`operator`、`conversion_operator`、`getter`、`setter`、`init_accessor`、`add_accessor`、`remove_accessor` | 显式 method/constructor 各一节点；普通体和 expression body 归 code，attribute/defaults/constraints 归 signature。接口和 abstract 声明可无实现，`code=None` 不表示提取失败 |
| Value | `field`、`constant`、`enum_member`、`property`、`indexer`、`event` | 多变量字段声明逐 declarator 建模并保留共同类型/attribute；initializer 归各 Value。源码 accessor（含 expression body 投影的 getter）为其 Callable 子节点；auto accessor 的声明可建节点但不制造 backing field/实现体 |
| ExecutionRegion | 每文件一个 `top_level_statements` | 保留顶层语句顺序，顶层 local functions 按下面粒度规则处理；不生成隐式 `Program` 或 `Main` |
| Extension | C# 14 的 `extension_block`；建议的 `inactive_region` 原文保留项 | extension block 记录 receiver 与约束并 contains 显式 extension members；inactive region 只表示未参与当前配置的文本，不假称已绑定的 Type/Callable |

**事实：** positional record 会合成属性、构造、相等比较等成员；顶层语句也会产生编译器入口。**建议：** 首期以显式源码对象为图成员，record 参数保留在 Type signature，不一并展开自动生成的 Value/Callable；普通 primary-constructor 参数也不按名称推断字段。若想把 positional property 独立显示，应作为单独产品决定并规定来源锚点。[record 语义](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/builtin-types/record)、[顶层语句](https://learn.microsoft.com/en-us/dotnet/csharp/fundamentals/program-structure/top-level-statements)、[extension block 规范](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/proposals/csharp-14.0/extensions)

**建议：局部粒度采用保守首版。** local function、lambda、anonymous method 和局部变量保留在最近外层 Callable/ExecutionRegion/Value 的内容中，内部调用也归该 owner；不创建匿名 closure、临时变量或 lowering 节点。对已证实的具名 local function 可保留内部稳定索引，作为 Repository External 目标；不能把调用回指外层方法制造自调用。匿名 delegate/function pointer 的实际目标未知时使用 Unresolved。C# 的 local function 是独立语言特性，采用此显示粒度是建议，不能称为 Python Q3 对 C# 的既有约束。[local functions](https://learn.microsoft.com/en-us/dotnet/csharp/programming-guide/classes-and-structs/local-functions)

### partial、重载与稳定身份

**建议：源声明身份与编译上下文身份分开。** 源节点使用 `csharp:repo:<escaped-relative-path>:<lexical-owner>:<kind>:<declaration-key>:<part-role>`；语言限定符、路径分隔符、大小写和转义规则固定。排除绝对路径、行号/偏移、随机 ProjectId、ISymbol.GetHashCode、metadata token、时间戳、工具安装路径。选择文件范围变化不得改变已有 ID；改文件路径/声明名按新增删除，不推断移动或重命名。

- 同名 C# overload 是不同方法，不像 TS overload signatures 合成一个节点。declaration-key 包括成员名称、显式接口限定、泛型 arity、规范参数类型语法和 passing mode；泛型参数按位置编码。参数名、默认值、返回类型及约束通常只改变 signature；转换运算符需把目标类型也纳入 key。为避免缺依赖导致 ID 改变，源码 key 不依赖解析成功后才得到的类型全名；别名等价不主动推断，类型拼写改变可能表现为删除/新增。该保守代价应写进规格。[方法签名规范](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/basic-concepts#75-signatures-and-overloading)
- partial type 按每个显式声明片段输出 Type，每片只拥有自己文件的成员和 source。Roslyn 的同一 type symbol 内部连接所有声明；不新造跨文件聚合节点，不按选中片段选“主声明”。一个确定 type symbol 对应多个合法 partial 声明时，结构型关系可映射到所有已确认片段，各自成为 Node 或 Repository External；这与多个互斥候选 symbol 的歧义不同。[ISymbol 的多声明语义](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.isymbol?view=roslyn-dotnet-4.13.0)
- partial method 的 defining/implementing declaration 分开保留源码节点，role 参与 ID；调用优先映射经 `PartialImplementationPart` 证明的实现片段。合法 optional partial 缺实现时，建议不输出已被编译期删除的调用及其参数求值调用，但完整源码仍参与 code 比较；required partial 缺实现不能假装合法擦除。generator 被关闭或源码成员清单不完整时，不能断言“确实没有实现”，应保留 Unresolved。partial property/indexer，以及较新语言版本的 partial event/constructor 同样需要配对规则。[partial members](https://learn.microsoft.com/en-us/dotnet/csharp/programming-guide/classes-and-structs/partial-classes-and-methods)、[成员版本限制](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/keywords/partial-member)
- 多处相同 namespace/partial 片段可用同 owner/key 的局部序号消重，明确插入同名片段可能造成保守替换；普通 overload 不能靠源码顺序分配 ID，否则调整方法排列就会串配。属性访问器的角色 `get/set/init/add/remove` 参与身份。
- 编译上下文用仓库相对 project path、TFM、configuration/platform 和有效 options 标识，进入会话/缓存；首期唯一上下文政策下不把随机上下文 ID 混进每个节点。共享源码多项目冲突不得碰巧保留一个结果。外部 metadata symbol 用 assembly identity、逻辑 package/TFM 来源和规范成员签名编码；可使用 DocumentationCommentId 辅助，但它会为空且不是 assembly 全局唯一键，不能独立充当 NodeId。[DocumentationCommentId](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.documentationcommentid?view=roslyn-dotnet-4.13.0)

### 自身内容、顺序、生成内容与坐标

**建议：** 使用 Roslyn tokens/结构投影生成可读规范 signature/code。普通注释和纯排版忽略；literal、raw/verbatim/interpolated string、attribute、nullable/checked/unsafe 和有意义 directive 保留。不能通过正则剥 whitespace，也不能删除所有 trivia：`#if/#define/#nullable/#line` 与 DisabledText 都在这条边界上。方法体变更只修改方法，getter body 只修改 getter，不能让 property/class 复制子体后重复 Modified。

**建议：** expression-bodied property/indexer 的 `=> expression` 是源码明确给出的 getter 实现，可投影为 getter 子节点，以 property 身份 + getter 角色定位；property 自身不再重复保存该表达式。保留 accessor 的声明角色集合与修饰符、将 body 只归子节点，能区分删除 setter 与修改 getter。纯 `get; set;` 只记录显式访问器声明，无实现体；事件只有显式 add/remove 才建访问器节点。文档注释先按普通注释不参与 diff，但保留原始 source；若后续将 XML 文档视为 API 契约，需另立规则。

**事实：** 同一片段中字段初始化器按文本顺序，partial 的不同片段相对顺序不确定。**建议：** Type 的 `extensions["csharp.initializer_order"]` 分别记录 instance/static initializer 的有序声明 ID，覆盖字段、auto-property 和 field-like event initializer 的交错；initializer 内容仍只归 Value。enum 未显式赋值时成员顺序影响值，也须保留有序 ID 或明确常量值投影。不能仅依赖无序 contains，也不要制造跨文件初始化顺序；普通方法重排不必改变父 code。[字段初始化顺序](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/classes#15561-general)、[enum 成员值](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/enums#204-enum-members)

**建议：** async/iterator 仍是原始 Callable，await/yield 和体内调用在源码层比较；不输出状态机 `MoveNext`、closure display class、自动 backing field、record 自动相等函数。图的 calls 表达静态源绑定，不表示该代码此时一定执行，迭代器的惰性执行尤其如此。[async 模型](https://learn.microsoft.com/en-us/dotnet/csharp/asynchronous-programming/task-asynchronous-programming-model)、[yield](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/statements/yield)

**事实：** 条件编译会跳过非活动区域，不能保证非活动文本是独立合法 C#。**建议：** 首期活动声明使用明确 parse options；inactive 文本原样保留在 Extension/owner 投影中并标明没有语义关系。它不是“已完整枚举全部配置对象”；若用户要求所有分支声明都独立显示，需另做有界配置分析，不能把所有符号一起置 true。file-based app 的 `#:` 指令由构建系统解释，不能看到 `#:package` 就安装依赖。[条件编译规范](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/lexical-structure#655-conditional-compilation-directives)、[file-based 指令](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/preprocessor-directives#file-based-apps)

**事实（本地差距）：** 现有 [SourceText](../../src-tauri/src/structure_review/source_text.rs) 支持 LF/CRLF、可选 CR 与 U+2028/U+2029，没有 NEL U+0085 规则；C# 和 Roslyn 识别 NEL 为换行。**建议：** helper 返回物理 UTF-16 offset/span、源内容摘要和原文锚点，Rust 验证后转 UTF-8 byte；如使用 LSP 行列，则先扩展显式换行配置，不能直接套 TS 的选项。`#line` 的 mapped filename/line 只用于展示，身份/范围校验必须使用原始 SyntaxTree 路径和物理 span。[C# 行终止符](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/language-specification/lexical-structure#632-line-terminators)、[Roslyn TextUtilities](https://github.com/dotnet/roslyn/blob/main/src/Compilers/Core/Portable/Text/TextUtilities.cs)

## 关系与四种目标边界

当前各语言并没有完全相同的关系目录：0018b 推荐表用 `code.inherits`，Python 实现实际发 `code.extends`；Rust 有 imports/宏关系，TS/Python 明确省略 import 边。C# 必须固定自己的 documented semantics，不能把推荐表的每一项都称为已实现的通用自动能力。以下建议以 [0018b](../specs/0018b-code-relationships.md)、实际 [Rust references](../../src-tauri/src/structure_review/rust_analyzer/references.rs)、[TS references](../../src-tauri/src/structure_review/typescript_analyzer/references.rs) 和 [Python references](../../src-tauri/src/structure_review/python_analyzer/references.rs) 为基线。

| 关系 | C# 建议映射与限制 |
| --- | --- |
| contains | 文件 → namespace/顶层对象，类型片段 → 显式成员，property/event → accessor；一对象只放一处 |
| `code.calls` | 显式 invocation、object creation/构造初始化器，以及经绑定确认的源码运算符/LINQ/collection initializer 调用；去重。virtual/interface 目标是静态声明，不展开所有运行时实现 |
| `code.references` | 已绑定到方法的 method-group/function-value 引用；不要将字段/属性数据访问和普通类型名混进来；`nameof` 是编译期名称，不当调用 |
| `code.member_of` | 成员指向经符号确认的语义类型；partial 可定位到全部合法声明片段，contains 仍只保留词法声明位置。extension method 的成员归属仍是定义它的静态类/extension block，不移进 receiver 类型 |
| `code.inherits` | class/record 的显式基类、interface 的显式基接口；不凭同名类型建立关系，不自动添加隐式 object 父类 |
| `code.implements` | type → 显式实现的 interface；成员级显式接口实现可作为后续细化，首期不建立全程序实现分派图 |
| `code.applies_to` | extension block 或传统 extension method → 经确认的 receiver type；开放类型参数接收者不能伪造具体 Type |
| `code.type_use` | 先沿用现有窄范围：字段/property/indexer/event 的类型中已确认且在图内的 Type（含泛型参数中的具名类型）；一般参数/返回类型/局部类型-use 不扩大提取。图外或未知 type-use 可暂省略，此项是明确范围，不代表不存在类型依赖 |
| `code.imports` | 首期不发。using/static using/alias 必须参与解析及配置内容比较，但 namespace 引入不代表运行时调用 |
| `code.reads` / `code.writes` | 首期不发；也不借自动 getter/setter call 边变相加入全面数据访问分析。赋值表达式中真正的调用仍提取 |

**事实：** `IInvocationOperation.TargetMethod` 和 `IsVirtual` 可区分静态声明目标与虚分派；`IDynamicInvocationOperation` 不给出同样的确定目标。`SymbolInfo.CandidateSymbols` 是未成功绑定时的候选，不是成功边；只返回一个 candidate 也不能升级为已确定。[Invocation](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.operations.iinvocationoperation?view=roslyn-dotnet-4.13.0)、[Dynamic invocation](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.operations.idynamicinvocationoperation?view=roslyn-dotnet-4.13.0)、[SymbolInfo](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.symbolinfo?view=roslyn-dotnet-4.13.0)

**建议：** 扩展方法先通过 `ReducedFrom` 回到静态声明，再通过 `OriginalDefinition` 归一泛型实例；不为 `F<int>` 和 `F<string>` 复制源码节点。LINQ query 用 operation/`GetQueryClauseInfo` 的实际绑定，不能硬编码 Enumerable 或假定 provider；query 中的 lambda 调用归原 owner。dynamic、反射字符串、未知 delegate/function-pointer dispatch 为 Unresolved。合法委托类型的 `Invoke` 不能冒充已经确定的用户方法。[IMethodSymbol](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.imethodsymbol)、[QueryClauseInfo](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.csharpextensions.getqueryclauseinfo?view=roslyn-dotnet-4.13.0)

| 目标状态 | 需要的证据 | 示例 |
| --- | --- | --- |
| Node | 绑定到本侧选集保留节点，目标类别合格且 source span 对应 | 选中 `C.cs` 中 `F(int)` 的调用；partial 实现文件被选中 |
| Repository External | 已确认是同侧仓库源码，但未选中或按已决粒度不建节点；ID 与完整选择时一致 | 未选中 `Utilities.cs` 方法；已确认但不独立显示的 local function |
| Dependency External | 已读取明确目标的标准库/平台 reference metadata 并确认 assembly/type/member 身份 | 标准库 `Console.WriteLine`；不能仅凭 namespace 判断来自库，也不因 NuGet 业务依赖已在本机缓存而纳入 |
| Unresolved | 缺引用、error symbol、候选歧义、动态绑定、上下文冲突或生成符号来源不足 | 只有 `Foo.Bar` 拼写、缺 generator 实现、缺 third-party 引用导致重载绑定失败 |

**建议：** 编译器合成且确有稳定逻辑身份的 record/默认构造目标可作为 Repository External，ID 用源类型 + 生成角色/签名，不用机器生成路径；不具备该证据则 Unresolved。未执行的 source generator 不能猜结果；源码中已有生成文件与本轮内存生成 document 要区分。外部关系不补造 graph-local node，当前 [graph-presentation](../../src/structure-review/graph-presentation.ts) 隐藏 External/Unresolved 的行为继续复用。

**错误规则：** 语法恢复树、选中文件读取失败、协议 EOF、helper 异常、超时、非法坐标/越界来源、无法满足已选静态配置范围都是硬失败。缺少业务依赖可以留下完整的支持范围内声明和逐关系 Unresolved；但未知配置可能影响 overload 候选集，不能仅因当前不完整 Compilation 返回一个 target 就说项目真实绑定确定。支持 profile 必须在已决执行与依赖边界内于分析前选定；不能以依赖缺失为名吞工具错误。

**测试代码：** 复用 `include_tests=false` 的“只排除明确测试专用”原则。建议只有可信项目元数据或已验证 xUnit/NUnit/MSTest attribute identity 才排除完整子树，未知/共享 helper 保留；不要按 `[Test]` 短名称猜测，也不要运行 test discovery。基础模式缺测试框架引用时不应误删同名业务方法。包需求预判若证据不足可保守准备包，不能为了省下载先漏掉源码。

## 支持包、跨平台与再分发

**建议的包组成：** Narrant helper、固定 Roslyn `Common`/`CSharp`，按 RID 的私有 runtime 或 self-contained 文件、协议/版本 manifest、全部许可证和 ThirdPartyNotices；当前路线不需要为 MSBuild 导入附带 BuildHost。可另带明确锁定且许可核实后的基础 reference profile，作为分析上下文，不能冒充所有项目 TFM。用户项目 SDK、workloads、其余 targeting packs、NuGet 包不因安装支持包而自动齐备；项目 SDK 不安装、不替换，第三方业务包不进入默认上下文。

**事实（含已排除候选）：** Roslyn `BuildHost-netcore` 与 `BuildHost-net472` 有相对部署布局要求，缺文件会抛错；这仍是原 MSBuild 候选的分发成本，不是当前包要求。`dotnet/runtime` 主仓库为 MIT，仍须保留许可并检查实际发布包的第三方通知，不能推导所有外部 targeting packs/SDK 资产都使用同一许可证。[BuildHost 部署源码](https://github.com/dotnet/roslyn/blob/main/src/Workspaces/MSBuild/Core/MSBuild/BuildHostProcessManager.cs)、[runtime LICENSE](https://github.com/dotnet/runtime/blob/main/LICENSE.TXT)、[runtime ThirdPartyNotices](https://github.com/dotnet/runtime/blob/main/THIRD-PARTY-NOTICES.TXT)

**建议：** 首期只沿用 Narrant 已有 macOS Apple Silicon（包 target `aarch64-apple-darwin`，.NET RID `osx-arm64`）和 Windows x64（`x86_64-pc-windows-msvc` → `win-x64`）。macOS x64、Windows arm64、Linux x64/arm64 是后续扩展，不是现有支持承诺；Linux 的 glibc/musl/native prerequisites 单列。不把 NuGet 的 framework 兼容表等同于 OS 清单。采用普通目录 self-contained 发布，暂不启用 trimming/Native AOT/single-file，减少 MEF、动态装载与 BuildHost 路径风险；实际产物仍须 smoke test。

**建议的固定版本策略：** 首轮原型可用 Roslyn `5.9.0`、`net10.0`，语言 profile 先固定 C# 14；这只是已存在的候选组合，不是已经验证的发布组合。支持包发布前还要锁定 helper 源提交/协议版本、完整 NuGet lock、runtime 精确 patch、RID、每项上游散列、reference-profile 版本/散列与许可证清单。不要只写 `.NET 10` 或 `latest`；参考 profile 的版本也不能由宿主 runtime 推断。

### 具体接入文件与扩展点

| 位置 | 实施内容（本轮均未修改） |
| --- | --- |
| 新 `src-tauri/src/structure_review/csharp_analyzer/` | `mod.rs` 实现 trait；负责输入来源边界、session/protocol、通用 DTO 校验、最终图构造与生命周期，不重新实现 C# 身份/内容/关系映射。中文注释；Rust 测试在模块 `tests/` 并按约定挂载 |
| 建议新 `tools/csharp-analyzer/` | 独立 C# helper 以纯 compiler API 和受控静态配置获取语法语义；完成类别、内容、身份、图成员与关系目标分类，输出统一接口的规范化构图 DTO，不接管 Git checkout、不导入 MSBuild。字段协议仍待设计；C# 测试也独立文件 |
| [structure_review/mod.rs](../../src-tauri/src/structure_review/mod.rs)、[application/mod.rs](../../src-tauri/src/structure_review/application/mod.rs)、[bootstrap.rs](../../src-tauri/src/tauri_app/bootstrap.rs) | 新 analyzer 装配、`.cs` language group、两侧文件分组、需要包的预判、图合并；保持现有取消、panic 和清理路径。新增语言不必先造通用插件注册框架 |
| [lsp/transport.rs](../../src-tauri/src/lsp/transport.rs)、[Windows Job](../../src-tauri/src/lsp/windows_process.rs) | 原生 helper 建议使用小型批量 JSON-RPC；抽取/复用已有 framing 和进程守卫，当前 Transport/ServerProcess 是 LSP 私有，不能声称直接可无改动调用。若用 LSP 则保留初始化/能力/请求语义并禁用 daemon |
| [source_text.rs](../../src-tauri/src/structure_review/source_text.rs) | helper span 到 UTF-8 的校验；需要 LSP 坐标时补 NEL 规则和独立测试，避免改变 Rust/Python 的既有规则 |
| [PackId/model](../../src-tauri/src/language_support/model.rs)、[storage](../../src-tauri/src/language_support/storage.rs) | `CSharp` / `csharp`，必需入口及历史包扫描枚举；校验 runtime/reference metadata 完整性，复用租约/事务 |
| [版本表](../../resources/language-support-packs.json)、[build](../../scripts/build-language-support-packs.mjs)、[publish](../../scripts/publish-language-support-packs.mjs)、[workflow](../../.github/workflows/language-support-packs.yml) | 新包 whitelist/入口选择/精确依赖来源/许可证收集/native smoke；现有脚本只将 `bin/` 标 executable，且 storage 只接受 `bin/runtime/licenses/control`，布置 helper apphost 时必须处理权限和相对布局。不得仅替换归档 URL |
| [前端包类型](../../src/language-support/types.ts)、[错误过滤](../../src/structure-review/use-structure-review.ts)、[包显示](../../src/components/language-support/language-support-packs.tsx)、[语言显示](../../src/components/structure-review/structure-changes-view.tsx)、[编辑器映射](../../src/components/review-reader.tsx) | 包名/恢复提示、`csharp` 语言标签、`.cs` Monaco 映射；关系/partial 长名展示。真正实现时仍需 Preview Harness 宽窄窗口和自动化验收 |

**事实（本地）：** 当前包构建/安装拒绝 symlink 和特殊文件，清单有逐文件散列，原生验证只在匹配 OS/CPU 上运行。**建议：** .NET 普通目录发布必须保留普通文件相对布局；仅在 macOS 生成 Windows 归档不算 Windows 验收，`--version` 成功也不能替代实际符号分析和 helper 进程回收。

## 取消、子进程回收、性能与缓存

**事实：** 独立 Roslyn LSP 支持 `--daemon-mode`，其 bootstrap 故意让 daemon 脱离原进程树并在客户端断开后 keepalive。当前 `main` 的自动项目发现另有默认最多 500 个项目的限制，超限可能先排除 `test/tests`。**建议：** Narrant 禁用 daemon，使用逐侧独立进程和显式 project/solution 列表；不继承会静默漏上下文的项目发现默认值。上述发现来自指定版包文档和当前源码，固定支持包必须复验一致性。[独立 LSP 包文档](https://www.nuget.org/packages/roslyn-language-server/5.12.0-1.26426.8)、[当前 README](https://github.com/dotnet/roslyn/blob/main/src/LanguageServer/roslyn-language-server/README.md)

**事实：** `OpenSolutionAsync`、`OpenProjectAsync` 接受 CancellationToken，但当前 MSBuild build manager 源码说明单项 submission 不能取消、其 RPC 层不支持取消，因而只在启动 build 前检查 token。上层 API 有取消参数不代表自定义 task 正在执行时可及时退出。[Workspace API 实现](https://github.com/dotnet/roslyn/blob/main/src/Workspaces/MSBuild/Core/MSBuild/MSBuildWorkspace.cs)、[BuildAsync 实现](https://github.com/dotnet/roslyn/blob/main/src/Workspaces/MSBuild/BuildHost/Build/ProjectBuildManager.cs)

**建议：** 每个固定侧建立专属 helper 生命周期；请求取消先传播 token，超过短宽限期后由 Rust 杀整个进程树/Windows Job Object 并 wait/reap。回收覆盖 helper 的所有后代；BuildHost、MSBuild node 和自定义 target 在当前路线本就不应启动。stdout 保持协议纯净，stderr 有上限；单文件、静态配置读取、全请求分别计时。取消的部分输出绝不能被缓存为成功图。

**事实：** Roslyn `Project.GetCompilationAsync` 会复用已有 Compilation，`GetDependentSemanticVersionAsync` 表示项目和引用项目的语义版本。**建议：** 可以在单侧会话内复用 immutable compilation、树和允许来源的 metadata，但持久缓存不能只用选定文件内容；必须包括 commit/side、project/TFM/config、Roslyn/pack/reference profile 版本、parse options、输入来源与禁止执行政策等。跨 before/after 可以共享只读 metadata bytes，不能共享同一可变 Workspace。[Project 缓存/版本 API 源码](https://github.com/dotnet/roslyn/blob/main/src/Workspaces/Core/Portable/Workspace/Solution/Project.cs)

**未验证：** 实际 cold/warm 加载时延、峰值 RSS、加载项目数、语义模型复用收益、常规取消延迟以及不协作 task 的强制退出延迟。无需逐 occurrence 发 LSP 往返是 helper 的架构优势推断，不能据此填写虚构性能数字。

**本地生命周期接入：** [应用层](../../src-tauri/src/structure_review/application/mod.rs) 现在两侧并行，每侧 Rust/TS/Python 分组并行；增加 C# 后不能再在每个项目中无限 fan-out。建议每侧一个专属 helper、项目按需建立 Compilation，限制同时活跃项目数。helper 批量返回已完成语言映射的构图 DTO，按源文件缓存语义模型，重复调用目标去重；日志/耗时/缺依赖清单放诊断封套，不塞入参与 diff 的 `extensions`。同 Snapshot 改 C# 配置、包版本或 reference profile 时必须使图缓存失效。

## 可执行最小实验与验收矩阵

### 实验 A：只用 compiler API 验证跨文件、重载、partial 与 dynamic

**未执行。** 本机 `command -v dotnet` 没有找到 PATH 入口，本轮没有安装工具。下列完整探针供已有 .NET 10 SDK 和目标 reference pack 的机器在临时目录运行。它只构建自有探针、使用内嵌合成源码，不加载被审查 `.csproj`，不 emit 或执行合成源码，不调用 MSBuildWorkspace/generator。探针自身 restore 会下载明确指定的 Roslyn 包；这不是建议产品自动 restore 用户仓库。

在仓库外创建空临时目录，把下列两段分别保存为 `Probe.csproj` 和 `Program.cs`。此处只是文档内实验材料，本轮没有在仓库新增这两个文件。

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>
    <RestorePackagesWithLockFile>true</RestorePackagesWithLockFile>
  </PropertyGroup>
  <ItemGroup>
    <Compile Include="Program.cs" />
    <PackageReference Include="Microsoft.CodeAnalysis.CSharp" Version="5.9.0" />
  </ItemGroup>
</Project>
```

```csharp
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;
using Microsoft.CodeAnalysis.Operations;

// 只接受调用者明确提供的 reference assemblies 目录，不能用宿主 runtime 代替。
if (args.Length != 1 || !Directory.Exists(args[0]))
    throw new ArgumentException("需要目标 reference pack 的 ref/net10.0 目录");
var references = Directory.GetFiles(args[0], "*.dll")
    .Select(p => MetadataReference.CreateFromFile(p)).ToArray();
if (references.Length == 0) throw new Exception("参考程序集为空");
var options = new CSharpParseOptions(LanguageVersion.CSharp14);
var a = CSharpSyntaxTree.ParseText("""
namespace Demo;
public partial class C {
    public static int F(int x) => x;
    public static int F(string x) => x.Length;
    partial void Hook();
    public void Run(dynamic d) { _ = F(1); Hook(); d.Unknown(); }
}
""", options, path: "A.cs");
var b = CSharpSyntaxTree.ParseText("""
namespace Demo;
public partial class C { partial void Hook() { _ = F(2); } }
""", options, path: "B.cs");
var compilation = CSharpCompilation.Create("Fixture", new[] { a, b }, references,
    new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary));
var errors = compilation.GetDiagnostics()
    .Where(d => d.Severity == DiagnosticSeverity.Error).ToArray();
if (errors.Length != 0) throw new Exception(string.Join("\n", errors.Select(d => d.ToString())));
var model = compilation.GetSemanticModel(a);
var type = model.GetDeclaredSymbol(a.GetRoot().DescendantNodes()
    .OfType<ClassDeclarationSyntax>().Single())!;
if (type.DeclaringSyntaxReferences.Length != 2) throw new Exception("partial 声明丢失");
var calls = a.GetRoot().DescendantNodes().OfType<InvocationExpressionSyntax>().ToArray();
var f = (IMethodSymbol?)model.GetSymbolInfo(calls.Single(c => c.ToString() == "F(1)")).Symbol;
if (f?.Parameters.Single().Type.SpecialType != SpecialType.System_Int32)
    throw new Exception("没有精确绑定 int 重载");
var hook = (IMethodSymbol?)model.GetSymbolInfo(calls.Single(c => c.ToString() == "Hook()")).Symbol;
var implementation = hook?.PartialImplementationPart ?? hook;
if (implementation?.DeclaringSyntaxReferences.Single().SyntaxTree.FilePath != "B.cs")
    throw new Exception("partial 实现未定位到 B.cs");
if (model.GetOperation(calls.Single(c => c.ToString() == "d.Unknown()"))
    is not IDynamicInvocationOperation) throw new Exception("dynamic 被错误提升为普通静态调用");

// 缺依赖时须保留声明，同时不能把候选或拼写当成确定方法。
var missing = CSharpSyntaxTree.ParseText(
    "class X { void M() { MissingLibrary.Do(); } }", options, path: "Missing.cs");
var incomplete = compilation.AddSyntaxTrees(missing);
var missingCall = missing.GetRoot().DescendantNodes().OfType<InvocationExpressionSyntax>().Single();
if (incomplete.GetSemanticModel(missing).GetSymbolInfo(missingCall).Symbol != null)
    throw new Exception("缺失依赖被误解析");

// NEL 与非 BMP 字符覆盖 Rust 坐标接入时不能忽略的差异。
var unicode = CSharpSyntaxTree.ParseText("class N { string S = \"😀\"; }\u0085class Z {}", options);
if (unicode.GetText().Lines.Count != 2) throw new Exception("NEL 行数不符");
Console.WriteLine($"Roslyn={typeof(CSharpCompilation).Assembly.GetName().Version}");
Console.WriteLine($"partial-parts={type.DeclaringSyntaxReferences.Length}; overload={f!.ToDisplayString()}");
Console.WriteLine("PASS: partial / overload / dynamic / missing-reference / NEL");
```

从该临时目录执行（把变量填成机器上**已存在**的、明确版本的 reference 目录）：

```sh
export NARRANT_REF_DIR='/absolute/dotnet/packs/Microsoft.NETCore.App.Ref/<exact-version>/ref/net10.0'
dotnet --info
dotnet restore Probe.csproj --source https://api.nuget.org/v3/index.json --packages ./packages
dotnet run --project Probe.csproj --no-restore -- "$NARRANT_REF_DIR"
```

记录 SDK/Roslyn/reference pack 精确版本和 `packages.lock.json`，第二次使用 `dotnet restore --locked-mode`。预期只有全部断言满足才打印 PASS；缺 reference、编译诊断、API 不兼容都失败。这个实验不验证 Narrant 图 ID、差异算法、LSP、项目加载或分发，仅证明原生编译器路径的最小技术假设。可执行方案依据 [CSharpCompilation.Create](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.csharpcompilation.create?view=roslyn-dotnet-4.13.0) 和 [MetadataReference.CreateFromFile](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.metadatareference.createfromfile?view=roslyn-dotnet-4.13.0)。

### 实验 B：项目导入与执行边界（已排除候选的实验设计）

**未执行；保留原候选实验设计，不作为当前或默认未来路线的实施步骤。** 原设计需要单独临时 fixture，不能在真实仓库试危险导入：准备一个 SDK-style solution，包含两个项目 ProjectReference、一个共享 linked `.cs`、两个 TFM；每个 TFM 用 `#if` 绑定到不同实现。将选定 `TargetFramework`、`Configuration`、`Platform` 传入 `MSBuildWorkspace.Create`，输出真实 project documents、parse options、metadata reference origins 和全部 WorkspaceFailed/diagnostics，断言所有源码都指向本侧原始路径。

另加一个仅写临时 marker 的 `BeforeTargets="Compile"` 自定义 target，以及分别写独立 marker 的最小 generator/analyzer。依次测试：纯 Compilation、MSBuild 导入、移除 AnalyzerReferences 后 GetCompilationAsync、显式 generator driver/分析器执行。预期纯 Compilation 不写任何 marker；MSBuild 的 marker 用于证实“设计时构建仍会执行项目逻辑”，generator/analyzer 各自仅在明确允许的路径运行。不得因为某次 marker 没出现就推断所有项目 tasks 均不会运行。再加可取消的长时间 fixture child，用超时/取消和 Windows Job/Unix 进程组验证回收。

还需分别测试“没有 obj assets，但缓存有包”“已有 assets 指向旧绝对路径”“缺 project SDK/targeting pack”“legacy csproj”“global.json/Directory.Build.* 位于仓库外父目录”。拒绝来源不明的旧 assets，不能从用户主 checkout 复制 obj 或源码来让探针通过。MSBuild 模式的通过条件包括未修改 tracked source/config，以及无未经允许的源码生成污染；只通过 Compilation 创建不够。

### 实验 C：LSP 备选的同题比较

**未执行。** 固定上述同一 fixture、TFM/reference 和选集，比较原生 helper、`roslyn-language-server 5.12.0-1.26426.8`、`csharp-ls 0.28.0`；OmniSharp 只有先确认 v2 payload/runtime/协议后才加入。记录 initialize capabilities、明确 project loading、documentSymbol、definition、可用的 call hierarchy、partial 多位置、metadata URI、dynamic 和断开后后代进程。缺方法应记 unsupported，不能返回空结果冒充支持；不要启用 daemon、自动 restore 或插件。评价依据是可证实的图事实、错误质量、启动/内存/取消，而非“支持 LSP”标签。

### 必须通过的验收矩阵

所有行当前均为 **待执行**，并非本轮测试结果。当前路线的执行反例应验证 helper 不触发 marker；实验 B 中主动运行已排除候选的设计不属于产品验收要求。

| 维度 | 正例与反例 | 通过条件 |
| --- | --- | --- |
| 支持范围 | `.cs` 的 C# 7.3、8–14 代表语法；C# 15 preview、`.csx`、Razor、file-based 指令单列 | 只承诺固定版本已验收范围；unsupported 明确，不把恢复 AST 当完整成功 |
| 对象 | nested types、delegate、record/primary ctor、各 accessor、event、operator、顶层 async、iterator、extension block | 六类映射稳定，无隐式 Program/MoveNext/backing field；无父子内容重复 |
| partial | 两文件只选其中一文件；definition/implementation 分离；optional erased call 含有调用参数；required/generator 缺失 | 图只含选集；实现定位有证据；不随选择改变 ID；缺生成上下文不假称擦除 |
| 身份 | 同名 overload 重排/插入，泛型参数改名，显式接口实现，两个 checkout 根、中文路径 | 无绝对路径/行号/随机 ID；独立 overload 不串配；纯参数名变化不删增 |
| 内容差异 | 方法体、accessor body、attribute/defaults 改动；expression-bodied property/indexer 与显式 getter；field/auto-property/field-like event initializer 交换，enum 隐式成员重排 | 修改落正确 owner；属性表达式只归 getter，不重复比较；初始化顺序可见；普通注释/排版不误报，literal/directive 不丢 |
| 绑定 | 同项目/跨项目、重载、ReducedFrom、generic、virtual/interface、自定义 LINQ provider | 精确静态声明，不把运行时所有实现合并；query 不硬编码标准库 |
| 不确定性 | dynamic、未知 delegate、候选重载唯一但绑定失败、缺引用、同名伪库 | Unresolved 保留；候选/名字非确证；数据读取不伪装 function reference |
| 四类边界 | 选中/未选中目标、stdlib/平台 references、本机 NuGet 缓存反例、生成符号、被排除测试 | Node/Repository External/Dependency External/Unresolved 准确；本机业务依赖不自动读入，不补造节点 |
| 项目 | `.sln/.slnx/.csproj` 静态配置、SDK-style/legacy、共享文件、多 solution、多 TFM、conditional Compile | 仅承诺已验收的静态覆盖；项目归属和上下文明确，不取第一个、不扁平合并，不启动 MSBuild 补齐 |
| 预处理/文本 | inactive 非法文本、#nullable、#line、UTF-8 BOM、CR/LF/CRLF/NEL/LS/PS、emoji | 原文/声明/坐标一致；物理路径不受 #line 伪路径影响；编码范围明确 |
| 工具链/配置 | 缺 SDK/runtime/refs/restore，污染父目录/环境、外向 symlink/junction、linked source | 不自动安装、不偷偷回退；无法证明上下文的模式明确失败 |
| 执行 | 自定义 build target、generator、analyzer marker，输出改 tracked source/config | 项目与依赖的执行入口均不触发 marker；工作区污染失败；不使用源码副本规避 |
| 两侧/取消 | 一侧失败、主动取消、refresh/close、异常退出、RPC EOF、卡住 task | 保留首个硬错误、取消 peer、所有后代结束后释放租约；无成功空图/迟到成功 |
| 支持包 | 候选两平台干净机器、缺文件、摘要错、runtime 不匹配、reference profile、使用中删除 | 无宿主 dotnet 仍能运行已声明模式；缺平台声明如实失败；native smoke 含真实查询和退出，最终目标矩阵另定 |
| 性能 | 1/10/100 项目，冷/热、只选一文件、大 partial 类型、重复调用目标 | 记录 wall time、RSS、读取/查询数、缓存命中、取消延迟；设有界并发，不宣称未测数字 |
| 应用/UI | Rust+TS/JS+Python+C# 混合，新增/删除 `.cs`，测试开关，长 partial/overload 标签 | 硬失败不展示不完整比较；缓存按政策失效；实现时补类型检查/Lint/build/测试和浏览器验收 |

## 分阶段交付与待决定的产品问题

| 阶段 | 可交付内容 | 进入下一阶段的门槛 |
| --- | --- | --- |
| P0 技术假设 | 在候选目标平台运行实验 A 与当前 helper 禁止执行反例；B/C 保留候选研究，确定 compiler/helper 版本、包许可来源、取消边界 | 有真实原始输出，验证不触发 MSBuild/generator/analyzer，并覆盖多 TFM 与 partial 反例；目标矩阵未定，未通过不写“已支持” |
| P1 最小图闭环 | 明确受控基础模式范围，helper + Rust adapter；完整选集声明、规范内容、稳定身份与已确认关系 | 现有 CodeGraph 校验/diff、四类边界、语法硬失败、内容归属全部通过；核心范围不依赖自有项目 SDK |
| P2 静态项目配置 | 单一明确 TFM、多项目 ProjectReference 与目标 references；仅列无需 MSBuild 导入即可证明的配置组合 | 不执行项目或依赖代码，不引入本机第三方缓存；不支持的配置与缺少声明可解释 |
| P3 支持包和应用 | `csharp` 两平台精确支持包、自动准备与租约、混合两侧图、设置/错误恢复/编辑器映射 | native smoke、完整许可清单、取消/崩溃/删除保护和应用验收；不手动触发 quality-gate |
| P4 扩展 | 多上下文显示/一致性求证、更多静态配置与源码形式 | 在已决边界内独立规格与实测；不预留默认启用 MSBuild/generator 的路线，不能把 LSP 自带能力直接升级为产品承诺 |

以下保留已决约束并列出仍需在实现规格中决定的细节；字段协议与目标支持矩阵尚未确定。

| 问题 | 推荐起点 | 另一选择及代价 |
| --- | --- | --- |
| 静态配置能承诺什么语义等级？ | 已决不允许 MSBuild 导入；明确受控 compiler API 模式能证明的项目与配置范围 | 复杂工程可能因配置不足失败，不能以“可选项目模式”重新引入设计时执行 |
| 标准库/平台 reference profile 如何提供？ | 已决默认只含仓库源码与明确目标的标准库/平台声明；具体 TFM、来源与版本仍待选定 | 不下载/restore 第三方，也不自动使用本机已有业务依赖；目标矩阵需实测，不能把 helper runtime 当目标引用 |
| 多 TFM/共享文件如何呈现？ | 首期明确选择单一适用上下文；歧义时报选择需求 | 全上下文需要图/展示/差异策略；只做关系并集会误导。后续可实现所有上下文一致才确认 |
| 源码粒度要不要展开 local functions、positional record properties、非活动分支？ | 局部体归外层；record 参数留 Type；非活动文本可见但不声称绑定 | 全部独立显示增加身份、条件分支和内容归属成本；与 TS/Python 粒度不必完全相同，但需明确 |
| 已生成内容的展示粒度？ | 已决禁止项目及依赖 generator/analyzer；跟踪的 `.g.cs` 按项目/选集规则正常提取，编译器隐式成员不建 Node 是建议 | 隐式成员的 External 身份与源码归属仍需规格；不通过运行 Razor/XAML 等生成链补齐 |
| 初始工程/版本与测试范围？ | 普通 `.cs`、SDK-style 优先；7.3/8–14 分级实测，测试仅按可靠证据排除 | legacy/Unity/Razor/preview/file-based 构建支持逐项增加；测试框架未解析时保守保留 |

### 与现行 ADR 的相容性及冲突

| 决定 | 本方案判断 |
| --- | --- |
| [ADR-0030](../adr/0030-use-extensible-code-object-kinds.md) | 六类别 + `csharp` native kind/语义扩展相容。为 record/property 新增公共枚举类别不必要；改变多父/跨文件聚合需显式重议图契约 |
| [ADR-0031](../adr/0031-reuse-review-head-for-structure-analysis.md) | 自有 helper/控制文件相容；复制 `.cs/.csproj`、每语言 checkout、跨侧共享可变 Workspace、daemon 逃离回收都冲突。MSBuild 可能写生成源码/配置，且当前租约最后检查不覆盖 untracked 文件；任意设计时构建不能天然满足固定视图，必须限制/检测写入或重议该模式，不能仅在最后 `git diff` 一次就声称解决 |
| [ADR-0032](../adr/0032-separate-language-support-packs-from-project-toolchains.md) | 私有工具 runtime、固定 Roslyn 包相容；按当前应用验证版本下载，不跟 latest。自动安装/替换项目 SDK、workload、Visual Studio/Mono 不符合边界。固定参考 profile 是独立待确认上下文，不能当项目 SDK |
| [ADR-0033](../adr/0033-bound-python-analysis-to-repository-and-standard-library.md) | 仅决定 Python；C# 的默认依赖边界由后续 ADR-0034 明确，不是自动扩展 Python 的实现规则 |
| [ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) | 原生 helper 完成语言映射并返回规范化构图 DTO；禁止项目/依赖代码执行，默认只读仓库源码与明确目标标准库/平台声明。原 MSBuild/restore/本机第三方缓存候选已被排除，字段协议及支持矩阵仍待定 |
| 0019 / 0021 的 LSP 技术决定 | 是 Rust/TS 适配器的已接受技术路线；公共 `LanguageAnalyzer` 不要求 LSP，因此 C# 原生 helper 不构成修改它们。不能为 C# 而偷偷替换既有 Rust/TS 后端 |
| 0018 / 0022 的失败和完整性 | 有界输入下完整抽取声明并用 Unresolved 表示缺上下文相容；硬错误成功空图、项目导入失败自动降级、只合并其他语言的成功图均冲突 |

## 本次验证范围

完成上述本地源码/契约阅读、官方文档/NuGet/一手源码联网核实和独立映射复核；执行了 `git rev-parse HEAD`、只读源码检查和 `command -v dotnet` 预检。PATH 中未找到 dotnet；未安装 .NET，未运行本文 C# 探针、Roslyn/LSP/MSBuild/restore，未执行 Cargo 或 quality-gate，未修改业务代码、创建 Issue/PR、提交或切换分支。本会话唯一文档输出是本文件。

实际重载/partial 绑定、禁用执行政策、多平台包内容、完整许可证闭包、工作区不被污染及进程回收仍是待验收项。本文给出接入建议及可追溯证据，不是已完成 C# 支持的声明。

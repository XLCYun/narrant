# Java 语言支持方案调研

后续工程草案：[公共进程协议](../specs/0024-native-analyzer-helper-protocol.md)、[语言配置与验收](../specs/0024a-native-language-analysis-profiles.md)。本文保留调研过程中的候选和历史待决项；已确认决策以 ADR-0034 为准，工程建议以草案和后续实测修正为准，未验收项不构成发布承诺。

共同输出语义已由 [ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 确认：helper 返回单张配置并集图，对象和已解析关系至少在一种已分析配置下成立，不保留配置归属，不新增配置切换/变体界面。必要的配置分析与兼容源码对象归并在 helper 内完成；无法正确归并按完整图契约失败。下文具体发现算法与后端能力仍须验证，不能把并集理解为穷举所有配置或直接同时启用互斥源码。

调研日期：2026-10-02。仓库比较基线：`6761aca`。状态：研究建议，未实施、未选定支持包发行版本；后续已决架构与范围见下。

> **后续已决范围：** [ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 确认 Java、Go、C#、C/C++ 各自使用原生 helper，在同一进程中取得语法与语义，通过统一接口返回规范化的 CodeGraph 构造 DTO。helper 负责语言特有类别、自身内容投影、稳定身份、图成员及 Node / External / Unresolved 关系分类；Rust 负责输入来源边界、协议、通用校验、最终 CodeGraph 构造与生命周期。跨进程传输不采用整个编译器 AST。
>
> 四门新语言均不执行被分析仓库及其依赖携带的构建脚本、插件、注解处理器或生成器；受控分析工具也不得借加载触发这些行为。默认输入为仓库源码与明确目标的标准库/平台声明，不为分析下载或 restore 第三方，也不自动纳入本机已有第三方缓存或环境；仓库已提交源码按项目和选集规则处理。Rust、TS/JS、Python 现状不变，Rust 可选的 `all-dependencies` 保留。具体字段协议、目标支持矩阵及后端版本仍待设计和验证；下文超出此范围的原研究候选不属于当前实施路线。

本文用 **事实** 标记已从仓库、一手文档或源码核实的行为；用 **推断** 标记据此得出的适配判断；用 **建议** 标记尚待产品/实现确认的方案；用 **未验证** 标记必须通过实验才能承诺的能力。`main`、`master`、`latest` 链接是本次核查的精确页面，不是固定版本承诺；正式实施必须锁定发布构件、依赖树、摘要与对应源码。上游 README、开发分支和已发布构件的能力不能互相替代。

## 推荐路线与决策边界

**建议：先验证“固定 Java 运行时 + Eclipse JDT Core DOM/AST 与 bindings + Narrant 专用批处理桥接进程”，以原生 `javac` 编译器 API 为主要备选。** 同一个后端同时提供声明范围与语义绑定，避免让 tree-sitter 与 LSP 两套模型再做位置对齐。第一阶段仅支持明确列出的 Java 版本和项目形态；JPMS、多源码根、目标标准库以及缺失依赖实验是正式选型门槛。

**选型澄清：JDT Core 与 javac 是实验阶段的对照和替代候选，不要求生产分析同时运行二者，也不设计 JDT 失败后自动切换 javac。** 实验后选择一个后端；若其无法满足必要能力，在实施选型时更换后端或明确限制支持范围。下文的 javac 对照命令和编译桥接探针属于研发验证，不代表用户每次分析都启动两个分析后端。

**用户已确认：不执行 Maven/Gradle/JDT LS 的自动构建项目导入，不运行仓库及依赖携带的构建脚本、插件、注解处理器或生成器。** 静态读取可证明的源码根、模块和版本配置，不等于执行构建。JDT LS 的完整构建导入是本次排除的候选，不再列为默认后续增强；只有用户明确改变此决定后才重新评估。默认输入也不扩大到任意已下载或用户提供的第三方 JAR。JDT LS 集成 JDT、m2e 和 Buildship，提供符号、调用层次、类型层次及 Maven 自动注解处理；这些功能也不证明 LSP 返回值包含 Narrant 的全部图模型信息。[JDT LS README](https://github.com/eclipse-jdtls/eclipse.jdt.ls/blob/main/README.md)

**推断：仓库源码 + 明确目标的标准库/平台声明可以提取源码里实际存在的声明，并解析一部分有完整依据的仓库/标准库关系；不能可靠复现任意 Maven/Gradle 项目的完整语义。** 父 POM、profiles、代码生成、平台 SDK、编译参数和依赖版本会改变可见符号。默认范围由后续 [ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 确认，具体提取完整性仍需实验；不是从 Python 的 [ADR-0033](../adr/0033-bound-python-analysis-to-repository-and-standard-library.md) 推导 Java 决定。

## 当前仓库的实际契约与适配影响

以下均为 **仓库事实**，来自本次读取基线源码，不把旧研究中的历史实验当成现行实现。

| 已读取实现 | 实际约束 | Java 接入的影响 |
| --- | --- | --- |
| [LanguageAnalyzer / AnalysisInput](../../src-tauri/src/structure_review/code_graph/analyzer.rs) | 同步返回完整 CodeGraph 或 AnalysisError；files 只决定输出；include_tests 默认 false | 后端选 LSP 或原生编译器 API 都合法；Java 特有 release/classpath 不塞进语言中立 AnalysisOptions |
| [CodeNode](../../src-tauri/src/structure_review/code_graph/node.rs)、[校验](../../src-tauri/src/structure_review/code_graph/validation.rs) | 六类、每节点语言/native_kind、一个 contains 父、所有 Node 目标必须存在 | 不增加 Java 专属公共枚举；包/模块的跨文件语义归属不能复制节点或形成多个包含父 |
| [diff](../../src-tauri/src/structure_review/code_graph/diff.rs) | signature/code/extensions 参与自身比较，source 不参与；contains/references 按集合比较 | 方法体不能污染父类；Java 初始化次序必须另存有序投影，不能依赖 contains 数组顺序 |
| [Rust 入口](../../src-tauri/src/structure_review/rust_analyzer/mod.rs)、[语法](../../src-tauri/src/structure_review/rust_analyzer/syntax.rs)、[关系](../../src-tauri/src/structure_review/rust_analyzer/references.rs) | 本地 ra_ap_syntax + rust-analyzer；配置依赖范围；真实定义证据与局部缓存；显式声明优先 | 借鉴职责划分，不把 Rust 的宏、trait、闭包和依赖策略当成 Java 规则 |
| [TS/JS 入口](../../src-tauri/src/structure_review/typescript_analyzer/mod.rs)、[身份](../../src-tauri/src/structure_review/typescript_analyzer/identity.rs)、[关系](../../src-tauri/src/structure_review/typescript_analyzer/references.rs)、[内容投影](../../src-tauri/src/structure_review/typescript_analyzer/syntax/content.rs) | Oxc + 固定 TLS/tsserver；多项目分别求证，冲突不取第一个；身份来自源文件及词法声明 | Java 同一文件参与多个 source set 时也需独立求证；不可复制 TS 的接口方法调用资格规则 |
| [Python 入口](../../src-tauri/src/structure_review/python_analyzer/mod.rs)、[语法](../../src-tauri/src/structure_review/python_analyzer/syntax.rs)、[关系](../../src-tauri/src/structure_review/python_analyzer/references.rs) | Ruff + ty；按项目分组；类/模块有序声明锚点；过滤测试后再次归并 Node 目标 | 有序投影可借鉴；函数内不建节点、固定 stdlib、路径测试规则都是 Python 特定决定 |
| [应用编排](../../src-tauri/src/structure_review/application/mod.rs)、[before 生命周期](../../src-tauri/src/structure_review/application/workspace.rs) | 两侧固定 Snapshot、语言组并行、任何硬失败取消同组工作、全部 join 后返回 | Java 进程/线程不能脱离回调继续用工作区；一侧空选集才可直接空图；仅删除 .java 也要准备工具 |
| [支持包模型](../../src-tauri/src/language_support/model.rs)、[服务](../../src-tauri/src/language_support/mod.rs)、[存储](../../src-tauri/src/language_support/storage.rs) | 精确版本、共享下载、原子安装、PackLease 与跨进程文件锁 | 下载应在借用工作区前完成；分析期间不能删包；分析器不能自行下载 JDK/JAR |

已读取规格 [0018](../specs/0018-structure-review-code-graph.md)、[0019](../specs/0019-rust-language-analyzer.md)、[0021](../specs/0021-typescript-language-analyzer.md)、[0022](../specs/0022-language-support-packs.md)、[0023](../specs/0023-python-language-analyzer.md)；对照 [0018a](../specs/0018a-code-object-kinds.md)、[0018b](../specs/0018b-code-relationships.md)、根 [GLOSSARY](../../GLOSSARY.md) 和 [domain 指南](../agents/domain.md)。规格含历史段落：例如 0019 后文还保留早期默认 build-script 行为，不能覆盖现行入口和前文“关闭 build scripts/proc macros”的约定。

### ADR 一致性与潜在冲突

- **ADR-0030：兼容。** 原生 JDT AST tag 只作内部实现；对外映射既有六类 + language/native_kind。[ADR-0030](../adr/0030-use-extensible-code-object-kinds.md)
- **ADR-0031：必须满足。** head 复用 Review Workspace，只多一个 before。复制 Java 源码/配置来拼 Eclipse 工作区、复制生成源码、另开每语言 checkout 都冲突。独立索引、桥接 JAR、控制元数据可以在外部临时目录；原工作区路径必须保持真实。即使工具元数据在外部目录，仍须验证分析器不写源码根。[ADR-0031](../adr/0031-reuse-review-head-for-structure-analysis.md)
- **ADR-0032：兼容但要澄清。** 随包携带运行工具的固定 JDK 类似 TS 包携带 Node；不等于安装/替换项目 JDK。自动跟随 latest、运行时 Maven 拉桥接依赖、为满足项目 toolchain 自动下载用户 JDK 都违背已验证精确能力与项目工具链分离的意图。[ADR-0032](../adr/0032-separate-language-support-packs-from-project-toolchains.md)
- **ADR-0033：仅决定 Python。** Java 的默认输入及执行边界由后续 ADR-0034 独立确认，不能由 Python ADR 推导；仍要守住工作区和生命周期契约。[ADR-0033](../adr/0033-bound-python-analysis-to-repository-and-standard-library.md)、[ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md)
- **Workspace Trust 已授权正常工具行为，不是 OS 沙箱。** 既有 ADR-0007 明确语言服务可能执行仓库配置，但不能覆盖本次用户明确排除构建导入的决定。源码被修改后恢复的瞬间也不在 ADR-0031 的前后检查证明范围内。[ADR-0007](../adr/0007-require-workspace-trust.md)、[ADR-0008](../adr/0008-treat-review-workspaces-as-disposable.md)
- **原生 API 不违背公共接口。** 0019 的 LSP 决定属于 Rust、0021 的固定 TLS 决定属于 TS/JS；公共 LanguageAnalyzer 未规定所有新语言必须 LSP。Java 后续按 ADR-0034 采用原生 helper，不把独立语言服务与 LSP 作为必经步骤。

## 候选后端比较

| 候选 | 已核实能力与入口 | 对 Narrant 的价值 | 主要代价与判断 |
| --- | --- | --- | --- |
| JDT Core `ASTParser` + bindings | 支持直接源码、批量文件、显式 classpath/sourcepath、语法版本选项与绑定恢复 | 同一 AST 可提取声明、范围、子对象及引用；不必先创建 Maven/Gradle 项目 | **首选实验对象**。Narrant 要自己提供项目模型；JPMS 与目标 API 需要专门验证 |
| JDT LS | Eclipse JDT + m2e + Buildship，支持 Java 项目及 LSP | 构建项目导入、工作区索引、增量能力成熟 | **完整构建导入已排除**。会扩大执行/下载/缓存/子进程边界，且仍需自定义 AST 导出；仅在用户改变决定后重新评估 |
| JavaParser + Symbol Solver | 独立 AST；组合源码、JAR、反射等 TypeSolver | API 直接、Apache-2.0 可选；不需要 Eclipse 工作区 | **轻量备选**。项目模型仍由 Narrant 提供，语法覆盖不等于编译器级重载与泛型准确性 |
| tree-sitter-java | 增量语法树与 Java grammar | 可在 Rust 侧快速提取声明和自身内容，适合语法预检 | **不单独担当语义后端**。grammar 没有 classpath、重载、类型推导或访问控制的解析器 |
| OpenJDK `JavacTask` + `Trees` + language model | `parse()`、`analyze()`、`Trees.getElement/getTypeMirror`，`--release` | 直接依据 Java 编译器语义，目标标准 API 控制清楚；不需要 LSP | **主要备选**。与 JDK 版本绑定；源码有错时的部分恢复、批量内存和取消需验证 |

能力依据分别见 [ASTParser](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/ASTParser.html)、[JDT LS](https://github.com/eclipse-jdtls/eclipse.jdt.ls)、[JavaParser](https://github.com/javaparser/javaparser)、[tree-sitter-java](https://github.com/tree-sitter/tree-sitter-java)、[JavacTask](https://docs.oracle.com/en/java/javase/25/docs/api/jdk.compiler/com/sun/source/util/JavacTask.html)、[Trees](https://docs.oracle.com/en/java/javase/25/docs/api/jdk.compiler/com/sun/source/util/Trees.html)。表中优先级是本调研建议，不是上游结论。

### JDT Core 的可行入口和真实缺口

**事实：** `ASTParser.setEnvironment` 支持没有 `IJavaProject` 的解析，要求调用者提供所需源码/二进制路径；原始字符输入还需 `setUnitName`。`setResolveBindings(true)` 才创建绑定；`setBindingsRecovery(true)` 产生的绑定可能不完整。批量 `createASTs` 共享环境并有 `IProgressMonitor` 参数。[ASTParser API](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/ASTParser.html)

**建议：** 专用桥接器只加载固定 JDT Core 及明确的运行依赖，显式构造每个 source set 的环境；不加载 m2e、Buildship、APT、项目插件或 Lombok agent。桥接器从传入的那一侧工作区读取 `.java`，所有配置覆盖只在内存中；不要为了建立 Eclipse project 复制源码或在工作区生成 `.project/.classpath`。这是拟实现的控制边界，**未验证**“该依赖闭包启动时完全不触发任意插件/文件写入”。

**事实：** 上游仍有公开问题报告 `ASTParser` 多模块解析及 `import module` 的缺口，报告构件为 `org.eclipse.jdt.core.compiler.batch_3.44.0.v20251118-1623`。问题存在不等于每个 JPMS 项目失败，也不等于后续发行版仍复现；但足以否定“支持 ModuleDeclaration 即支持完整多模块语义”的推论。[JDT Core issue #4912](https://github.com/eclipse-jdt/eclipse.jdt.core/issues/4912)

**建议：** 把多模块作为选型淘汰条件之一。先测同批次两个 named modules、`requires transitive/static`、非导出包与 `import module`。若公开 API 无法表达隔离而只能把所有源码根合成一个 classpath，不得因此产生越过模块可读性/导出边界的确定关系；在选型阶段改选 `javac` 或明确标为不支持这种项目配置，运行时不自动切换后端。不要依赖 JDT `internal` 包来偷偷补齐后再声称稳定 API 方案。

### JavaParser 与 tree-sitter 的边界

**事实：** JavaParser README 宣称 Java 1.0–25，给出 `3.28.2` 示例；本次读取的开发分支 `ParserConfiguration` 已出现 `JAVA_26`、`CURRENT = JAVA_26`。两处信息不一致说明开发能力与发行版不能混用；不得把“有枚举值”视为该版本所有语言特性/语义已完备。[README](https://github.com/javaparser/javaparser)、[ParserConfiguration.java](https://raw.githubusercontent.com/javaparser/javaparser/master/javaparser-core/src/main/java/com/github/javaparser/ParserConfiguration.java)

**事实：** `JavaParserTypeSolver` 以 package 根目录作为源码根，缓存解析文件、目录与符号，默认配置使用 `BLEEDING_EDGE`；源码提示大仓库应限制缓存。`JarTypeSolver` 读现有 JAR/class 层次。`ReflectionTypeSolver` 默认读取运行中的 JRE，默认 `jreOnly` 主要按 `java.`/`javax.` 过滤。因此不能用默认 ReflectionTypeSolver 把支持包运行时的 Java 25 API 当作目标 Java 8 API。[JavaParserTypeSolver](https://raw.githubusercontent.com/javaparser/javaparser/master/javaparser-symbol-solver-core/src/main/java/com/github/javaparser/symbolsolver/resolution/typesolvers/JavaParserTypeSolver.java)、[JarTypeSolver](https://raw.githubusercontent.com/javaparser/javaparser/master/javaparser-symbol-solver-core/src/main/java/com/github/javaparser/symbolsolver/resolution/typesolvers/JarTypeSolver.java)、[ReflectionTypeSolver](https://raw.githubusercontent.com/javaparser/javaparser/master/javaparser-symbol-solver-core/src/main/java/com/github/javaparser/symbolsolver/resolution/typesolvers/ReflectionTypeSolver.java)

**事实：** tree-sitter-java 的 grammar 包括 package、module、record、compact constructor、class/interface permits、注解类型、lambda 等语法；这个仓库提供的是 grammar，并没有 Java 编译环境或符号求解服务。[grammar.js](https://raw.githubusercontent.com/tree-sitter/tree-sitter-java/master/grammar.js)

**推断：** JavaParser 适合在无构建模式下做直接、可控的解析，但要以与 JDT/javac 对照的重载/泛型/模块语料验证准确性。tree-sitter 可以做快路径或诊断工具；若它接到 LSP 定义查询仍需语义后端与严格目标归并，维护两套树的成本可能超过收益。首版不建议同时引入。

### javac 作为主要备选

**事实：** `JavacTask` 把 parse、analyze、generate 分成不同阶段；`Trees` 可以在 TreePath 上取得 Element 与 TypeMirror。Narrant 可以调用 parse/analyze 后提取模型，不调用 generate；不必执行 `mvn compile` 或 `gradle classes`。[JavacTask](https://docs.oracle.com/en/java/javase/25/docs/api/jdk.compiler/com/sun/source/util/JavacTask.html)、[Trees](https://docs.oracle.com/en/java/javase/25/docs/api/jdk.compiler/com/sun/source/util/Trees.html)

**建议：** 以标准 `javax.tools.JavaCompiler`、`com.sun.source.*`、`javax.lang.model.*` API 实现实验，避开 `com.sun.tools.javac.*` 内部实现。使用固定显式参数，`-proc:none`，不接收仓库原样拼接的任意 compiler flags，显式禁止 compiler plugins。候选优势是 `--release` 将语法与目标标准 API 一起约束；缺点是旧版/新版/preview 只在该 JDK 支持范围内可用。JDK 25 手册明确 `--release` 只支持当前和有限数量旧版，`-source` 本身只限定语言规则；注解处理可用 `-proc:none` 禁用。[javac 25 参数](https://docs.oracle.com/en/java/javase/25/docs/specs/man/javac.html)

**未验证：** 错误源码下能否收集足够有效节点；`ErrorType` 与缺失方法如何被安全降为 Unresolved；多模块 sourcepath 与同名类隔离；单次 analyze 的最大内存；中断时是否响应线程 interrupt。不能用“官方编译器”替代这些集成实验。

## 已排除的 Maven/Gradle 导入会做什么

### Gradle：模型请求也会求值项目代码

**事实：** Gradle 的初始化/配置阶段执行 `settings.gradle(.kts)` 与项目 build script；顶层语句在这些阶段立即执行。Tooling API 能请求项目/依赖/源码目录模型、下载所需 Gradle 分发，并且始终使用 Gradle daemon，模型请求也在这个常驻进程内运行。Buildship 使用 Tooling API 导入项目。[构建脚本执行](https://docs.gradle.org/current/userguide/writing_build_scripts.html)、[Tooling API](https://docs.gradle.org/current/userguide/tooling_api.html)

**事实：** JDT LS `GradleProjectImporter` 会选 wrapper/指定/本地/default distribution，构造 Buildship BuildConfiguration，再调用 `gradleBuild.synchronize(monitor)`；配置包含 Gradle Java home、user home、init scripts、arguments 和 offline。源码中的 `--no-daemon` 添加逻辑被注释，不能靠这个选项声称没有常驻进程。[GradleProjectImporter.java](https://raw.githubusercontent.com/eclipse-jdtls/eclipse.jdt.ls/main/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/GradleProjectImporter.java)

**事实：** Gradle `--offline` 仅让依赖解析使用缓存，缺失缓存时失败；它不是禁用脚本求值的模式。[Dependency Caching / Offline mode](https://docs.gradle.org/current/userguide/dependency_caching.html#sec:offline-mode)

**推断：** 不能把“只 import、不 build”“offline=true”“校验 wrapper”当成不执行项目代码的保证。脚本、plugins、init scripts 仍可能有副作用；Tooling API 下载 Gradle 分发与依赖解析也是不同下载路径。这些是当前排除此路线的原因；只有用户明确改变决定后，才重新评估项目执行策略、受控 Gradle user home、网络策略以及 daemon 生命周期，单纯关闭 LSP 父进程不足以满足所有子进程退出要求。

### Maven：POM 是声明式，导入器并不只是 XML 读取器

**事实：** JDT LS Maven importer 调用 m2e `configurationManager.importProjects`，随后更新项目。m2e 的 lifecycle mapping 可指定 `runOnConfiguration=true`，使 Maven mojo 在导入/配置更新时执行；不是只在用户显式 build 时执行。[MavenProjectImporter.java](https://raw.githubusercontent.com/eclipse-jdtls/eclipse.jdt.ls/main/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/MavenProjectImporter.java)、[m2e 插件兼容机制](https://eclipse.dev/m2e/documentation/m2e-making-maven-plugins-compat.html)

**事实：** Maven 支持 `.mvn/extensions.xml`、build extensions 与插件 `extensions=true`，它们扩展运行时组件与 classloader；m2e 还允许对 Maven 插件 ignore、execute 或委托 project configurator。并非每次 Maven 导入都会执行任意 lifecycle goal，具体取决于 m2e 版本、项目与 mapping；但“POM 是 XML，所以导入必然无执行”不成立。[Maven extensions](https://maven.apache.org/guides/mini/guide-using-extensions.html)、[m2e lifecycle mapping](https://eclipse.dev/m2e/documentation/m2e-execution-not-covered.html)

**事实：** 当前 JDT LS `Preferences` 默认 `importGradleEnabled=true`、`gradleWrapperEnabled=true`、`gradleAnnotationProcessingEnabled=true`、`importMavenEnabled=true`，Maven/Gradle offline 默认均为 false。不得用默认启动后再补设置的顺序证明导入阶段安全。[Preferences.java 构造器](https://raw.githubusercontent.com/eclipse-jdtls/eclipse.jdt.ls/main/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/preferences/Preferences.java)

**已决边界与实现建议：** 不调用 Maven/Gradle/JDT LS 构建导入器，不执行仓库及依赖携带的构建脚本、插件、注解处理器或生成器。配置发现只静态读取明确支持的 POM 字段、仓库内 parent/modules 和显式 source roots；记录无法求值的 property/profile/extension，不能把简化 POM 当成 effective POM。Gradle 仅可把常见目录当候选，不能通过解析字符串可靠求出任意 Groovy/Kotlin 构建配置。读取静态配置与明确目标标准库/平台声明仍可研究，任意现有第三方 JAR 不纳入当前路线。

不执行构建导入对项目分析的影响如下；这些缺口不会让源码 AST 提取本身必须依赖 Maven/Gradle，但会影响编译上下文和关系的完整性：

| 上下文 | 不执行构建导入的缺口 | 可采用的无执行补足方式 |
| --- | --- | --- |
| 源码根与测试 source sets | 插件或脚本新增的目录无法自动完整发现 | 可证明的静态目录、用户明确提供的源码根；未知配置不能假装已解析 |
| 项目/模块图 | 条件子项目、source sets 间依赖与模块路径可能不完整；POM modules 不等于 JPMS 模块图 | 分别读取仓库内模块声明和可证明配置，接受显式 modulepath；无法保证隔离时拒绝该配置 |
| 目标 release 与平台 | 继承属性、profile、toolchain 脚本可能改变语言版本及标准 API | 静态可确定的 release 或显式目标配置，并提供对应标准 API；不能沿用运行工具的 JDK 猜测 |
| 第三方依赖 | 无法自动获得完整版本、传递依赖、classpath/modulepath 顺序 | 默认不下载、restore 或纳入已有业务 JAR；缺少允许范围内证据的相关绑定保留 Unresolved |
| 生成源码与成员 | 不运行生成器，未存在于快照中的源码或 Lombok 成员不可用 | 分析快照内已有的显式源码；缺少生成目标时保留不确定关系，不伪造生成成员 |

## 运行工具的 JDK 与目标项目的 Java 版本

应将以下三个值独立记录进分析上下文及缓存键，不能只放一个 `javaVersion`：

1. **工具运行时**：用于启动桥接器的固定 Java executable、厂商、完整构建版本与摘要。
2. **语法与语言规则**：`source`/compliance/release、preview 开关；按 source set 记录。
3. **目标平台 API**：目标 JDK 标准库、`--release` 签名、Android/Java EE 等平台库以及 JPMS 设置。

Gradle daemon JVM、Maven 执行 JVM 与 wrapper 属于已排除的构建导入路线，当前分析不会启动它们；静态配置中的 Java toolchain 信息只能作为待确认的目标平台输入，不能触发工具链下载或构建。

**事实：** JDT LS README 当前最低运行时要求 Java 21，同时宣称可编译 Java 1.8–25 项目；本次读取的 JDT Core manifest 是 `Bundle-Version: 3.48.0.qualifier`，`Bundle-RequiredExecutionEnvironment: JavaSE-17`。两者不是同一个运行时要求，开发分支 manifest 也不代表所有传递依赖的最低 JDK。[JDT LS requirements](https://github.com/eclipse-jdtls/eclipse.jdt.ls#requirements)、[JDT Core manifest](https://raw.githubusercontent.com/eclipse-jdt/eclipse.jdt.core/master/org.eclipse.jdt.core/META-INF/MANIFEST.MF)

**建议：** 支持包固定一个经测试的运行 JDK（实验可从 Temurin 25 开始），不会替换用户的 `JAVA_HOME`。目标 Java 8/11/17/21/25 的标准 API 必须独立控制；例如 Java 8 源码中的 `List.of` 不能因为桥接器运行在 25 上就解析为有效标准库调用。仅改 source/compliance、继续使用运行 VM bootclasspath 不能证明这一点。JDT 的目标 API 签名实现与 javac `--release` 做对照后再决定是否需要随包提供多版本签名/读取用户已有目标 JDK。

**建议：** 标准库/平台声明只承诺明确测试的目标版本矩阵；不匹配时给出明确不支持/需要配置的错误，不能默默按更高 JDK 解释。若使用用户已有 JDK，按 [ADR-0032](../adr/0032-separate-language-support-packs-from-project-toolchains.md) 区分支持包与项目工具链，不自动安装/修改项目 JDK。目标声明如何提供仍需验证，不因此纳入用户环境中的第三方业务依赖。

## Java 特有语义及图模型约束

### package、module、record、sealed

**事实：** JDT AST 有 module/package 节点；`RecordDeclaration` 自 JLS16 API 支持 record components、类型参数、接口与源码体；`TypeDeclaration` 支持 class/interface 的 `permits` 列表。[DOM/AST 包](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/package-summary.html)、[RecordDeclaration](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/RecordDeclaration.html)、[TypeDeclaration](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/TypeDeclaration.html)

**建议：** `module-info.java` 的 requires/exports/opens/uses/provides、package-info 注解必须被自身内容覆盖。包横跨文件，不把一个文件里的 `package p` 当作拥有全仓库 `p` 包对象内容。record component 与隐式 accessor/constructor 区分；源码没有声明范围的隐式成员不能伪造一段 Code Object Content。sealed 的 `permits` 不是继承边，声明中的 extends/implements 才建立相应关系；是否新增 Java 专属 `java.permits` 语义应显式决策。

### 方法、重载、泛型、匿名类与 lambda

**事实：** JDT `IMethodBinding.getMethodDeclaration()` 将参数化/原始泛型方法引用归到泛型声明，lambda 则返回 functional type 的 single abstract method。`IBinding.getKey()` 追求可预测与尽量稳定，但格式未定义；局部声明 key 依赖 block index，匿名类 key 依赖出现次数，恢复绑定也会有 key。因此有 key 不能证明绑定完整、也不能证明跨编辑身份稳定。[IMethodBinding](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/IMethodBinding.html)、[IBinding](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/IBinding.html)

**建议：** 稳定身份由 Narrant 自己定义：仓库相对文件 + 所属源码声明链 + native kind + 名称 + 不依赖外部解析结果的形参类型重载槽。不要把返回类型、行号、绝对路径或 JDT raw key 直接当最终身份。方法实例先归到声明；泛型参数重命名要规范化，擦除签名只作判重依据之一，不能把所有参数化信息从自身内容中删除。接口/抽象方法即使没有方法体也保留 Callable；default/static/private interface methods 按实际声明处理。

**建议：** local class、anonymous class 如独立建模，身份使用外层稳定对象和结构锚点，明确插入同构兄弟时可能退化为新增/删除。本文首选粒度是显式局部/匿名类独立、lambda 留在拥有者内容中；lambda 不能因 `getMethodDeclaration()` 返回 SAM 而与接口方法合并成同一对象。调用边表达静态绑定的被调用声明，不枚举潜在动态 override 实现；method reference 不等同立即调用。reflection、ServiceLoader 和注解驱动框架的运行时对象注入不是确定的静态调用事实。

**建议：** recovered/null binding、缺失依赖影响的重载选择以及错误类型一律走明确 Unresolved，不能按同名、import 拼接或候选第一个符号生成确定边。可确认的本地声明与不受错误影响的局部绑定仍可保留，但程序异常、解析器崩溃、协议失败不能转换成成功空图。

### 注解处理、Lombok 与生成源码

**事实：** Lombok 在 javac 路径中作为 annotation processor 改写代码，在 Eclipse/ECJ 路径中通过 agent 修改编译器类。仅读取 `@Data` 并不会自动获得真正的 getter/setter。[Lombok Execution Path](https://projectlombok.org/contributing/lombok-execution-path)

**事实：** vscode-java 的配置声明默认启用从项目 classpath 加载 Lombok processor，并把生成的 getter/setter/constructor 是否进入 document symbols 作为单独选项，默认不包含。这直接说明“documentSymbol 有/没有一个方法”不是源码对象清单的充分证据。[vscode-java package.json：lombokSupport / includeGeneratedCode](https://raw.githubusercontent.com/redhat-developer/vscode-java/master/package.json)

**已决执行边界：** 不执行 annotation processor、Lombok agent、delombok 或仓库及依赖携带的 source generator。已提交并处于固定工作区的生成 `.java` 是实际源码，可按项目和选择范围分析；未提交的 `target/generated-sources`、Gradle `build/generated` 与用户其他 checkout 的产物不应静默进入快照语义。没有生成成员时保留相关不确定关系，不从 Lombok 注解推造确定成员。原研究中的启用生成器模式已排除，不属于默认后续增强。

### 测试发现

**事实：** JUnit Jupiter 区分 `@Test`、参数化/重复测试、test factory、test template、`@Nested` 等，并允许组合/元注解继承测试语义；部分测试方法由继承获得。[JUnit 6.1.3 annotations](https://docs.junit.org/6.1.3/writing-tests/annotations.html)

**建议：** Java 的 include-tests 不能只按文件名 `*Test.java` 或单个 `@Test` 字符串决定。source-set 元数据（main/test）、有完整依据的 JUnit/TestNG 等注解、JUnit3 继承规则应形成独立可测试分类；文件路径只是限定范围的约定。若框架库缺失，精确 import 只能在用户明确接受额外“按拼写过滤”规则后用作测试约定，默认仍保留；它不能冒充解析成功的图关系。自定义组合注解、动态测试与构建自定义 source set 的范围要写清；不运行测试引擎、不执行测试发现插件。具体框架清单及弱证据时是否排除需产品决定。

## 建议的完整对象与关系映射

本节全部是 **建议的 Java 适配约定**，尚未被接受为规格。它采用“显式源码对象 + 静态绑定声明”的范围；后端可解析的所有 AST/隐式符号不必成为 Code Object。

### 六类 Code Object

| 通用类别 / native kind | Java 输入 | signature / code / 包含归属 |
| --- | --- | --- |
| Container / `file` | 有保留对象的选中 `.java` 文件 | 名称用仓库相对路径；signature/code 为 None；完整原文仅进 source。空白/普通注释文件不造空节点 |
| Container / `package_fragment` | 本文件显式 package；包括 package-info.java 的注解 | 只代表该文件中的包声明片段，保留 package 名和注解；可包含本文件顶层类型。不是跨文件合并的唯一包节点；不创建目录节点 |
| Container / `module` | module-info.java 中显式 open/module 声明 | 模块头进 signature；requires/exports/opens/uses/provides 分为下面的指令子对象；不把别的选中文件强行移动到它下面 |
| Type / `class`、`interface`、`enum`、`annotation_type`、`record` | 顶层、成员、具名局部类型 | 修饰符、类型参数、父类型与 permits 进 signature；成员剔除。接口/抽象类有源码事实，不因不能实例化而排除 |
| Type / `anonymous_class` | 显式匿名类体、枚举常量专属类体 | name 可为 None；由外层对象持有，显式方法/字段为子节点；父表达式保留 new、实参和匿名类锚点，不能抹掉构造行为 |
| Callable / `method`、`constructor`、`compact_constructor`、`annotation_member` | 每个源码方法/构造器、record 紧凑构造器、注解成员 | 每个 Java 重载各一个 Callable，不能按 TS/Python overload 组合并；签名保留返回值、throws、类型参数、形参、注解；注解成员的 default 表达式也进 signature。body 单独 code；abstract/native/接口无体方法及注解成员 code=None，空体方法 code 有空块 |
| Value / `field`、`enum_constant`、`record_component` | 显式字段、枚举常量、record 头部组件 | `int a=f(), b=g()` 拆成 a/b 两节点，分别拥有初始化器；共享修饰符/类型投影到各声明。组件不同时生成隐式字段/访问器三份对象；常量匿名类为其子对象 |
| ExecutionRegion / `static_initializer`、`instance_initializer` | 显式 static `{}` / 实例 `{}` 块 | 各块直接归 Type；body 为自身 code；构造器仍是 Callable。块的执行位置由类型的有序锚点记录 |
| Extension / `import`、`module_directive` | 显式 import/static import/wildcard；模块指令 | 保留规范声明文本，确保只改 import 或 exports 也可比较。Extension 表示源构造，不能塞一个原始 AST 大树 |

普通局部变量、参数、模式变量不另建 Value；lambda 不建独立 Callable/ExecutionRegion。它们留在最近建模的 Callable、Value 或 ExecutionRegion 中，内部调用继续遍历。这里选择局部/匿名类独立而 lambda 内联，是 Java 的建议粒度；与 Python Q3/Q6 不同，需要产品确认。Java 25 compact compilation units、module imports、preview features 不因 grammar 有节点就自动纳入首版：若被纳入，隐式顶层类需专门身份/内容方案；否则明确报不支持，不能解析到一半成功。

### 当前 Code Relationship 的精确映射

当前代码用 `code.extends`，而早期 0018b 建议表写 `code.inherits`；Java 应先与当前实现一致，并在未来规格统一名称，不能擅自重命名所有语言。

| 当前 semantic | Java 推荐含义 / 方向 | 不可混淆的边界 |
| --- | --- | --- |
| contains（非 reference 字符串） | 文件→声明、类型→成员、owner→局部类型/匿名类 | 每节点一个声明位置；语义所属包/模块可以不同于包含位置 |
| `code.calls` | 源码调用拥有者→编译期已绑定的 method/constructor；包括显式 this/super 与 new | 接口/抽象方法可以是已确定的静态声明目标；不声称最终动态实现。对隐式构造器无 Node 时按生成目标规则处理 |
| `code.references` | 已确认 `Type::method`、`obj::method`、`Type::new`→Callable 声明 | 方法引用创建函数值，不等于当场调用；SAM 方法不是 lambda 自身身份；普通字段读取不冒充此边 |
| `code.extends` | 子类→显式父类；接口→显式父接口 | 不为 implicit Object/Enum/Record 自动补全所有继承边；sealed permits 不是此方向 |
| `code.implements` | class/enum/record/匿名类→显式接口 | 不因方法同名自动推断实现；是否新增方法级 overrides 另议 |
| `code.member_of` | 成员→其语义拥有类型；有可靠模块/包证据时可表达额外归属 | 不复制跨文件对象；局部类的词法拥有者和成员语义拥有者分开 |
| `code.type_use` | 字段/record component→显式声明类型中的可确认类型声明 | 数组降到元素类型；泛型原型/参数按声明证据处理；类型变量不冒充真实 Type 节点。首版不扩成所有参数/返回值/局部变量的类型使用图 |
| `code.imports` | Extension/import→可唯一确认的导入类型/静态成员或确证的包 | wildcard 不枚举全包；单个 static import 可能引入多个重载，全部确证时可保留多条，否则 Unresolved；requires 不是 import |
| `code.applies_to` | 首版不使用 | Java 没有对应 Rust impl-block 的独立适配对象，不能给每个方法强加此边 |
| `code.reads`、`code.writes` | 首版不提取 | 与当前适配器范围一致；赋值仍在 code 中；`rust.macro_use`、`jsx.uses_component` 也不套用于 Java |

module requires/exports/opens/uses/provides 的完整源文本已经由指令对象比较；若未来新增 `java.requires`、`java.provides` 或 `java.permits`，需定义静态含义及方向，不当作 ServiceLoader/Spring 的运行时调用。静态方法调用解析与运行时 dispatch 在 Java 语言规则中是不同阶段。[JLS 15.12](https://docs.oracle.com/javase/specs/jls/se25/html/jls-15.html#jls-15.12)

关系按 `(source, semantic, target)` 去重，重复调用不产生计数变化。所有查询按自身内容的 owner 分派，不能因为进入 lambda/匿名类后忘记遍历，也不能将独立子方法的调用重复算给父方法。

### 身份：源码身份与后端绑定分开

建议源码节点 ID 采用转义后的 `java:repo:<relative-file>:<lexical-owner-chain>:<kind>:<name>:<overload-slot>`；这是示意格式，最终编码须碰撞测试。保持以下规则：

- 文件使用 `/` 相对路径；同一源码被几个 source sets 纳入时仍只创建一个节点，不把 source-set 的扫描顺序、classpath 指纹或临时项目名放进 NodeId。语义项目标识用于求证/缓存。同包同类在不同仓库文件不会相撞。
- 普通方法的 overload slot 只从**声明语法**的有序形参类型产生，局部泛型参数按声明位置 alpha-normalize；参数名、返回类型、throws、可见性、注解变化保留同一个 ID 并改变 signature。varargs/数组的身份可归一化，声明文本仍保留区别。不能依赖成功解析时的全限定类型、失败时换短名。类型拼写或参数类型改变可保守视为删除/新增，不推断移动/重命名。
- Java 合法重载按签名区分；泛型实例先用后端的 declaration binding 对应回显式源声明，再映射源码 ID。擦除用于二进制成员区分及验证，不删除 signature 中的泛型/界限/类型注解。泛型擦除、override-equivalent 与 bridge method 不意味着一个源声明应该膨胀为多个 Callable。[JLS 擦除](https://docs.oracle.com/javase/specs/jls/se25/html/jls-4.html#jls-4.6)、[JLS 方法签名](https://docs.oracle.com/javase/specs/jls/se25/html/jls-8.html#jls-8.4.2)
- 局部同名声明/匿名类/初始化块需 owner 内的声明类目和相同结构锚点序号，不用行号、UTF-16 offset 或整段 body 哈希。修改 body 保持 ID；插入无法区分的同构兄弟可能使序号漂移，必须记录这一保守局限。
- `IBinding.getKey()` 只作当前会话内关联依据，不能直接持久化为 Narrant 身份。跨 AST 批次先比较 declaration binding，再经路径和声明索引归一化；找不到源声明不能凭 FQN 猜一条 Node。
- 默认范围内的 binary External 建议使用 `java:stdlib:<target-profile>:<module>:<binary-type>#<member-descriptor>` 或对应明确平台声明的稳定身份，不能使用机器绝对路径。原研究的 `java:dependency:<artifact-origin>:...` 保留为已排除的第三方输入候选参考，不代表当前会读取 `.m2` 或其他业务 JAR。来源存在同名冲突而无法确认时保持 Unresolved；内容摘要进入缓存，具体身份编码仍待协议设计。

### 自身内容、执行顺序与生成对象

Java 字段初始化和初始化块有规定的执行顺序，enum constant/record component 的顺序也有语义。将所有子声明剔除而仅留下 contains 集合会漏报次序变化。[JLS 字段初始化](https://docs.oracle.com/javase/specs/jls/se25/html/jls-8.html#jls-8.3.2)、[JDT Initializer](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/Initializer.html)

**建议：** Type 的比较 code 保存 static/instance 两个有序初始化投影，以字段/块 ID 占位；enum constants 保留有序 ID；record 头用有序组件 ID 代替独立子组件全文。顺序投影也可放入具有明确含义的 extensions，但不在 signature/code/extensions 三处重复存相同事实。普通方法声明重排无差异；交换两个有副作用的字段或字段/块的顺序必须产生类型内容差异。字段初始值改动只改字段，方法 body 改动只改方法，父 type 的锚点保持不变。

显式局部类/匿名类从父 code 中替换为稳定锚点，保留外围控制流、构造参数和表达式求值位置；source 始终保留完整原文。record component 独立为 Value 后，其类型/注解归自己；父 record 保留组件次序。显式紧凑构造器不能伪装成一段源码中不存在的带参数构造器文本。

默认构造器、record accessor/equals/hashCode/toString、enum values/valueOf、编译器 bridge methods 均不因后端列出就创建源码 Node。已确认的隐式调用目标若有稳定 owner+descriptor，且 owner 来自本侧仓库，可作为 **Repository External** 并用独立 generated 身份；owner 来自明确目标标准库/平台声明时为 **Dependency External**。没有充分身份或来源依据则 Unresolved。JDT 特别说明默认构造器未必被标 synthetic，不能只靠 `isSynthetic()` 过滤；需结合源范围、`isDefaultConstructor()`、`isSyntheticRecordMethod()`。[IBinding](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/IBinding.html)、[IMethodBinding](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/IMethodBinding.html)

规范化必须用 AST/词法器，而非删空白/注释的正则；保留字符串、text block、数字、运算符、注解和有意义的类型语法。Java Unicode escape 在词法切分前处理，连注释边界也可能受影响；文本块缩进可能改变值。纯 Javadoc/普通注释保留在展示 source，首版不另做文档语义 diff。[JLS 词法翻译](https://docs.oracle.com/javase/specs/jls/se25/html/jls-3.html#jls-3.2)、[JDT IScanner](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/compiler/IScanner.html)

JDT `char[]` 范围为 Java 字符索引；Rust 为 UTF-8 字节。桥接器必须输出可验证的原文 UTF-16 范围或直接映射后的字节范围，统一绑定到同一份 source/hash；不能按字符数直接切 UTF-8。复用 [SourceText](../../src-tauri/src/structure_review/source_text.rs) 的边界校验，但 Java 对 Unicode escape 的原始/翻译后位置及 CR/CRLF 的规则需独立验证，不能抄 Python 或 tsserver 的换行配置。[ASTNode 范围](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/ASTNode.html)

## 已选文件、项目模型与不完整上下文

### 四种目标结果

| 观察到的证据 | 结果 |
| --- | --- |
| 唯一合格源码声明，存在于本侧已选且未排除对象表 | `Node(NodeId)` |
| 唯一合格声明在同侧仓库中，但未选中或按明确对象粒度未建节点；不属于被排除测试 | `External`，`SymbolScope::Repository`；源码声明与选中时使用同一个 ID |
| 已确认来自明确目标的标准库/平台声明，且能给稳定身份 | `External`，`SymbolScope::Dependency` |
| null/recovered/error type、缺依赖导致的重载不确定、歧义/配置冲突、无法证实来源 | `Unresolved`；只保留观察到的引用名，不猜 External |

被 include_tests 排除的目标不通过 External 边重新暴露；其 body 也不能混入父内容。ExternalSymbol 描述是可读信息，不参与 diff；不能拿展示名称反向证明身份。Java `isFromSource()` 只能说明源码来源，不能证明在仓库内；包名前缀 java/com/org 也不能证明 Dependency/Repository。

### 项目输入的建议优先级

1. 读取仓库内明确的静态 source-set/release 元数据或用户在应用中给出的明确分析配置；路径相对元数据所在目录解释。POM 字面值/仓库内 parent/modules 可支持，外部 parent/BOM、条件 profile、未知 property、插件产物不可假装算出 effective model。
2. 对没有可靠模型的普通非模块化源码，可使用清楚记录的 conventional profile（如一个明确源码根），按 package 与相对路径一致性验证；不要将整个 monorepo 全部根拼成一个 classpath。已发现但无法解释的项目配置不能悄悄被“无配置默认”覆盖。
3. 源码根、模块图、测试 source set、编码、release、classpath/modulepath 各自属于一个有效分析上下文；多个上下文指向同一源码时分别求证，目标冲突即 Unresolved，源码节点不重复。
4. 目标身份索引需涵盖被引用的上下文声明。先列路径再按需解析；如果 JDT standalone 无法提供可验证来源/声明位置，则批量解析必要上下文并建立 binding→源声明索引，评估成本，不能仅凭名字查仓库中的第一个同名类。语义上下文范围与图成员范围始终分离。
5. 本侧 workspace 之外的 source roots、父目录配置、symlink/junction 逃逸要明确拒绝；目标标准库/平台声明按单独确认的来源校验，不能据此放开第三方业务依赖。内存覆盖可以引用原始路径，不生成清洗后的配置副本。Android SDK、Java EE/Jakarta 不属于 Java SE stdlib，仅在明确目标平台范围内处理；Kotlin/Scala 业务字节码也不因本机已有便进入上下文。

**建议的可用性分级（内部处理，不新增当前未存在的分析 notes UI）：**

- 缺少业务 JAR 或未运行 Lombok：仍能提取的合法源码对象完整输出，不受影响的仓库绑定保留，其余 Unresolved；不能把“一个依赖缺失”当作零对象。
- 缺少首版承诺的固定标准库、无法实现目标 release/模块隔离、上下文越界或已选文件不可读：明确 AnalysisError。若未来提供 repository-only Java 模式，另定其可接受范围，不在运行中自动降档。
- 语法错误、AST MALFORMED/RECOVERED 导致无法完整提取支持对象、未知语法版本、桥接协议/进程/超时/取消/校验失败：硬失败，不成功返回部分图。
- 普通语义诊断与硬失败需逐类区分：缺类型/歧义不是全图语法错误，存在一个非空 binding 也不排除同位置有歧义/不可访问诊断。后端问题码与范围应参与接受确定关系的内部判定。[CompilationUnit.getProblems](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/CompilationUnit.html#getProblems())

**后续已决依赖范围：** 默认只使用仓库源码与明确目标的标准库/平台声明，不扫描用户 `.m2`/`.gradle` 缓存、不纳入已有第三方 class/JAR、不下载或 restore 业务依赖。原研究中“只读明确选定且带摘要的第三方 dependency closure”不属于当前路线。已提交到仓库的源码仍按项目与选集规则归属；两侧采用同一允许规则，分别读取本侧内容，不拿 head 的上下文替代 before。

### Java 测试过滤的首版建议

优先使用已确认的 test-only source set；共享 main/test 根只有全部上下文都证明 test-only 才排除。无模型时 `src/test/java` 可以作为用户明确接受的约定，`Test.java`/`*Test`、名叫 test 的方法不能默认整类排除。框架绑定可覆盖 JUnit4 `org.junit.Test`、Jupiter 的 Test/ParameterizedTest/RepeatedTest/TestFactory/TestTemplate、TestNG class/method Test，以及有证据的 JUnit3 TestCase；自定义组合注解只沿可验证的注解链推导。不运行 JUnit Platform discovery，因为“发现”不保证不加载扩展代码。[JUnit4 Test](https://junit.org/junit4/javadoc/4.13/org/junit/Test.html)、[Jupiter 注解](https://docs.junit.org/6.1.3/writing-tests/annotations.html)、[TestNG 注解](https://testng.org/annotations.html)

默认保留未知、混用和共享辅助代码；选项为 true 只恢复原选集中的测试对象，不把仓库所有测试加入图。包准备阶段如果只有文件后缀而无法判断 semantic test-only，宁可准备 Java 工具再过滤，不能错误地跳过必要支持包。

## 固定支持包、平台与再分发

**建议的最小组成：** 固定 Java runtime/JDK、Narrant Java bridge JAR、固定 JDT Core/ECJ 与运行时依赖闭包、协议/引擎版本元数据、目标标准库策略所需固定资料、许可证/NOTICE/源码获取信息。构建机可以用 Maven 解析并锁定工具自身的依赖；用户机器只运行已安装产物，不在分析时使用 Maven 下载桥接器依赖。不捆绑用户项目的 Maven/Gradle cache、任意第三方 JAR 或项目 JDK。

| 构件 | 核实的许可/平台证据 | 打包建议与未验证项 |
| --- | --- | --- |
| JDT Core / JDT LS | JDT Core LICENSE 为 EPL-2.0；JDT LS 源码头同样标明 EPL-2.0 | 保留许可证与 notice，按确切分发构件提供相应源码获取信息；逐个核对传递依赖，不能只写一个“Eclipse 许可” |
| JavaParser | 上游允许 LGPL 或 Apache 二选一 | 若采用建议明确选择 Apache-2.0，并继续核对 Guava/Javassist 等依赖 |
| tree-sitter-java | MIT | 保留版权与许可；若引入还要核对 tree-sitter runtime 许可 |
| Eclipse Temurin | 官方 FAQ 指明 GPLv2 + Classpath Exception；支持列表有 macOS Apple Silicon、Windows x64 | 适合当前目标平台候选；保留二进制内完整 legal 文件与对应源码分发安排，不使用未核实的 Oracle JDK 商用许可替代 |
| OpenJDK | GPLv2、Classpath Exception 与包内独立组件附加条款 | Classpath Exception 不是删除运行时源码/notice 义务的许可；正式发布按选定二进制逐项核查 |

依据：[JDT Core LICENSE](https://github.com/eclipse-jdt/eclipse.jdt.core/blob/master/LICENSE)、[JDT LS 源文件许可](https://raw.githubusercontent.com/eclipse-jdtls/eclipse.jdt.ls/main/org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/managers/MavenProjectImporter.java)、[JavaParser 许可选择](https://github.com/javaparser/javaparser#license)、[tree-sitter-java LICENSE](https://github.com/tree-sitter/tree-sitter-java/blob/master/LICENSE)、[Temurin FAQ](https://adoptium.net/docs/faq#is-eclipse-temurin-free-to-use)、[Temurin 平台](https://adoptium.net/supported-platforms)、[OpenJDK LICENSE](https://raw.githubusercontent.com/openjdk/jdk25u/master/LICENSE)、[OpenJDK 附加许可说明](https://raw.githubusercontent.com/openjdk/jdk25u/master/ADDITIONAL_LICENSE_INFO)。以上是发行设计所需的许可证证据，尚未形成某个实际 Java 支持包的完整再分发审计。

**建议：** 按 macOS aarch64 / Windows x64 各自产物固定下载 URL、大小、SHA-256、引擎/bridge/protocol 版本。JDK 的目录、symlink、executable bit、macOS 签名与 Windows DLL 加载必须与现有支持包安装器逐项适配，不能把一个官方压缩包直接嵌入就视为可运行。`jlink` 缩小运行时应放到后续：需证明保留 JDT 所需模块、目标系统类/签名和许可证后才可替代完整 JDK。

### 现行支持包机制需要的具体适配

已实际读取 [版本目录](../../resources/language-support-packs.json)、[构建脚本](../../scripts/build-language-support-packs.mjs)、[model.rs](../../src-tauri/src/language_support/model.rs)、[storage.rs](../../src-tauri/src/language_support/storage.rs)：目前只有 rust、typescript-javascript、python；平台只有 `aarch64-apple-darwin` 与 `x86_64-pc-windows-msvc`。Java 是新增独立 PackId，不扩大平台承诺。

当前 `packageDirectory` 拒绝链接/特殊文件，仅以 `bin/` 前缀标记 executable，其他文件统一 0644。安装器只接受 `bin/`、`runtime/`、`licenses/`、`control/` 路径前缀，归档上限 256 MiB、解压上限 1 GiB、最多 20,000 文件；不能在未知大小时假定完整 JDK+JDT Core 依赖闭包满足限制。

**建议布局：** `runtime/jdk/` 保留经验证的运行时结构；`runtime/java/bridge.jar` 和固定依赖 JAR；`control/java-tools.json` 记录协议/引擎/目标 API profile；`licenses/` 保存工具及每个依赖的许可、notice、源码获取信息。`PackId::required` 至少验证实际 java 入口、运行库、bridge、引擎 JAR、profile 元数据与许可证，不能仅验证一个启动器。

打包时只对固定工具归档内允许的链接做可审计的物化，拒绝外部逃逸；调整权限登记以保留 `runtime/jdk/bin/java` 等真实可执行文件，不能把 JDK 只复制一个 java 二进制到 `bin/` 后丢掉相对运行库。完整 legal 目录不能为了消除 symlink 而遗漏。jlink 产物是否还具有目标 release 资料、JRT/zip/JDT 必要模块需实测；当前尚未锁定 JDK 构建号、JDT 发布号或依赖闭包，不能据本文直接发布 Java 1.0.0。

**原生 smoke 门槛：** 两平台均通过安装服务下载/校验/解包后，从实际 PackLease 路径启动 Java、初始化 bridge、解析最小 record/跨文件调用、读取固定标准库、退出并验证无残留进程。仅 `java --version` 成功不够。共享下载任务及进度、删除租约、历史版本发现和失败重试沿用现有模型；现有下载是完整归档流式下载，不包含增量或断点续传能力。

## 取消、进程、性能与缓存建议

**建议：** 一侧一次受控 Java 子进程；输入为经 Rust 确认来源边界的工作区、选定路径、source-set 上下文与能力版本，输出为规范化 CodeGraph 构造 DTO 或明确错误；Rust 校验后构造最终图。桥接进程采用分帧协议，stdout 只放协议，stderr 放受限日志，具体字段与编码尚未定稿。取消先协作通知并设置 `IProgressMonitor`，随后有界等待，再终止进程树并 join I/O 线程；超时不能返回空图。JDT 的 monitor 参数只提供取消接入点，不是有界退出保证，Windows/macOS 都需实验。

**建议：** before/after 可以按共享工作区契约并发，但不共用可变 Java 环境、绝对路径缓存或进程内静态解析器；每个进程做内存限制，先测两侧峰值再决定子项目并发度。JDT bindings 会占用额外内存，持有 AST/binding 会延长保留；批量处理可共享解析工作。[ASTParser 性能说明](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/ASTParser.html)

**建议：** 首版跨分析调用只缓存规范化输出及固定工具信息；单次不可变分析会话内可缓存 source/AST/binding。缓存键包括 Repository/Review/Snapshot side/revision、选定路径集、分析选项、source-root/module 图、目标标准库/平台声明指纹和 bridge/backend 版本。仓库上下文变化、修改 target release、切换测试过滤都必须失效。当前不引入构建导入产生的 Eclipse index/Gradle daemon 缓存，不能复用另一侧未校验的字节码或源码。

### 与现有进程机制的具体衔接

现有 [LspClient](../../src-tauri/src/lsp/mod.rs) 有 initialize/shutdown 的 LSP 语义；[transport](../../src-tauri/src/lsp/transport.rs) 的子进程守卫与 I/O 实现是私有的。原生 bridge 不能只因使用 Content-Length 就直接套 LspClient。最小选择是 bridge 实现固定的 LSP 生命周期 + 私有批量 analyze 请求，或在确有共用需求时提取一个小的 framed-process 模块；选型实验决定，公共 LanguageAnalyzer 不受影响。

Unix 进程组与 [Windows Job](../../src-tauri/src/lsp/windows_process.rs) 已有整树回收机制，Java 应复用同等保证，不退化成单个 Child.kill。JVM 无响应、输入输出管道阻塞、读写线程 panic 都要有有界清理；操作取消保留 cancelled 类型，不能在真实错误之后只检查 token 就覆盖原始故障。进程退出、临时控制目录清理与工作区 lease 释放按此顺序完成。

启动使用支持包 java 的绝对路径、参数数组和受控工作目录；清除或显式覆盖 `CLASSPATH`、`JAVA_TOOL_OPTIONS`、`JDK_JAVA_OPTIONS`、`_JAVA_OPTIONS` 等会注入参数/agent 的环境，不经用户的 jenv/asdf shell shim。仅设置这些变量并不构成 OS 级文件/网络隔离；仓库路径、允许来源及插件加载也需验证。

每侧可用一个 JVM 顺序处理多个 project profiles，解析器/绑定环境按 profile 隔离，完成一个项目后释放 AST；若引擎状态无法可靠隔离再改独立进程。保留两侧并发，对语言×source sets 的组合增加有界调度，不无上限启动 JVM。测量冷启动、首解析、总耗时、peak RSS、对象/引用数、上下文文件读取数、缓存命中率、取消到回收延迟；未有测量前不承诺比 JDT LS/JavaParser 快多少。

首版缓存限于一次不可变分析会话和应用已有 Snapshot 内存结果；不增加持久 graph store。source/AST/绑定结果只在单次会话内复用；跨调用的 Snapshot 结果可包含 Unresolved，但键必须带实际有效配置与依赖可用性。两侧不能共享可变 AST 或 Eclipse workspace。目标 profile、外部 artifact 集合和引擎版本变化使缓存失效，即使源码没变。

## 具体接入位置与实施顺序

以下是未来代码改动清单，本次没有修改这些文件。

| 位置 | 最小改动与扩展点 |
| --- | --- |
| `src-tauri/src/structure_review/java_analyzer/`（新增）与 Java helper | Rust 实现 LanguageAnalyzer、输入来源边界、协议、通用校验、最终图构造与生命周期，测试在模块 tests/；helper 负责 Java 类别、syntax-content / identity / references、图成员与目标分类，返回规范化 DTO。具体目录与协议待实施设计 |
| [structure_review/mod.rs](../../src-tauri/src/structure_review/mod.rs) | 导出 java_analyzer |
| [application/mod.rs](../../src-tauri/src/structure_review/application/mod.rs) | `LanguageAnalyzers`/with_java、`prepare_analyzers`、`analyze_side`、`SideFiles`、`LanguageGroup`、language_group/coverage 加 Java；两侧同选项、失败互相取消、图合并验证 |
| [tauri_app/bootstrap.rs](../../src-tauri/src/tauri_app/bootstrap.rs) | 装配缺包/测试过滤路径与生产 Java analyzer；工具来自支持包租约，不能从被审仓库 `.tools` 取 |
| [language_support/model.rs](../../src-tauri/src/language_support/model.rs)、[storage.rs](../../src-tauri/src/language_support/storage.rs) | PackId::Java、slug、required、历史目录扫描；JDK 权限、布局、大小/文件数量验证 |
| [language-support-packs.json](../../resources/language-support-packs.json)、[构建脚本](../../scripts/build-language-support-packs.mjs)、[发布脚本](../../scripts/publish-language-support-packs.mjs) | 精确版本/摘要/许可与工具闭包；Java 独立分支；原生完整 smoke；不增加 latest 安装 |
| [支持包 workflow](../../.github/workflows/language-support-packs.yml)、[脚本测试](../../scripts/tests/language-support-packs.test.mjs) | Java 产物构建/权限/安装/协议验证矩阵；遵守“不手动触发 quality-gate” |
| [前端 PackId](../../src/language-support/types.ts)、[包展示](../../src/components/language-support/language-support-packs.tsx)、[Tauri 错误适配](../../src/infrastructure/tauri-structure-review-adapter.ts) | 包 union/名称、supportPacks 错误校验与设置管理；使用真实后端 requirements，避免只按当前文件猜工具 |
| [结构视图](../../src/components/structure-review/structure-changes-view.tsx)、[图投影](../../src/structure-review/graph-presentation.ts)、[reader](../../src/components/review-reader.tsx) | Java 名称/kind/图卡片；reader 已有 java→java 映射，不能把编辑器高亮误说成结构分析已经支持。External/Unresolved 继续按现行显示策略 |

Java 配置若需 source roots/目标 profile，放在 Java adapter 的类型化配置及 application settings 快照，一次分析固定后给两侧使用；各侧实际 POM/release 的不同仍是该 Snapshot 源码状态。第三方业务依赖不进入当前默认输入。不要临时发明一个泛化插件注册框架，也不要为 Java 修改公共 NodeId/CodeNode 枚举。

先通过下节 P0–P3 的后端实验，再固化 Java 规格和必要 ADR；然后按“对象与内容→语义及边界→固定支持包→两侧应用及 UI”交付。规格中写清支持/不支持的输入，不能用技术实验成功直接替代产品范围确认。

## 可执行的最小验证实验与验收矩阵

### P0：无构建的 JDT Core 绑定探针（本次未运行）

前提是**另行准备并锁定**固定 JDK 和包含 JDT Core/ECJ/运行依赖的完整 JAR classpath；这不是让被审仓库运行 Maven 的命令。设置 `NARRANT_JAVA_HOME` 为该 JDK 根，`NARRANT_JDT_CP` 为这些固定 JAR 的绝对 classpath（macOS 用冒号、Windows 用分号）。将下面单个 Python 脚本保存到临时目录并运行 `python3 probe.py`。它只生成合成 fixture 和自己的桥接探针，不复制任何被审仓库源码，不运行 Maven/Gradle/项目 main。

脚本执行固定 javac 编译探针，再执行固定 Java+JDT：分别在两个不同绝对目录分析相同源码，并增加上下文文件到 AST 回调列表。这里 javac 只编译探针自身，不是与 JDT 一起分析用户项目的第二个生产后端；生产桥接器应预先编译打包。输出应归档为实验记录；这是完整可执行配方，**尚未编译/执行验证**，不是已发布工具或完整 Code Graph 提取器。

```python
import os
from pathlib import Path
import subprocess
import tempfile

# 验证显式工具路径，避免经宿主的 jenv/asdf 自动选工具。
jdk = Path(os.environ['NARRANT_JAVA_HOME']).resolve()
cp = os.environ['NARRANT_JDT_CP']
suffix = '.exe' if os.name == 'nt' else ''
java, javac = (jdk / 'bin' / (name + suffix) for name in ('java', 'javac'))
assert java.is_file() and javac.is_file(), '需要完整、已固定的 JDK'
env = dict(os.environ)
for key in ('CLASSPATH', 'JAVA_TOOL_OPTIONS', 'JDK_JAVA_OPTIONS', '_JAVA_OPTIONS'):
    env.pop(key, None)

# Java 桥接只输出观察值；非空 binding 不等同生产可接受的确定关系。
bridge = r'''
import java.nio.file.*;
import java.util.*;
import org.eclipse.jdt.core.JavaCore;
import org.eclipse.jdt.core.dom.*;

/** 仅观察 AST 和绑定，不构建 CodeGraph，也不执行 fixture。 */
public final class Probe {
    /** 同时记录 recovered 状态，禁止只凭 key 非空宣称解析成功。 */
    static String binding(IMethodBinding value) {
        if (value == null) return "null";
        IMethodBinding declaration = value.getMethodDeclaration();
        if (declaration == null) return "missing-declaration";
        return declaration.getKey() + ";recovered=" + value.isRecovered()
            + ";declarationRecovered=" + declaration.isRecovered();
    }

    /** 固定语言级别；故意使用运行 VM 库，供 P1 检查目标 API 泄漏。 */
    public static void main(String[] args) throws Exception {
        Path root = Path.of(args[0]).toRealPath();
        List<String> rows = new ArrayList<>();
        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        Map<String, String> options = new HashMap<>();
        JavaCore.setComplianceOptions("17", options);
        options.put(JavaCore.COMPILER_PB_ENABLE_PREVIEW_FEATURES, JavaCore.DISABLED);
        parser.setCompilerOptions(options);
        parser.setResolveBindings(true);
        parser.setBindingsRecovery(true);
        parser.setStatementsRecovery(false);
        parser.setEnvironment(new String[0], new String[]{root.resolve("src").toString()},
                              new String[]{"UTF-8"}, true);
        String[] files = Arrays.stream(args).skip(1)
            .map(name -> root.resolve(name).toString()).toArray(String[]::new);
        String[] encodings = new String[files.length];
        Arrays.fill(encodings, "UTF-8");
        parser.createASTs(files, encodings, new String[0], new FileASTRequestor() {
            /** 保留诊断与语法 flags，以免恢复树被误当成完整提取。 */
            @Override public void acceptAST(String file, CompilationUnit unit) {
                String relative = root.relativize(Path.of(file)).toString().replace('\\', '/');
                rows.add("FILE " + relative);
                for (var problem : unit.getProblems()) {
                    rows.add("PROBLEM " + relative + " " + problem.getID()
                        + " " + problem.getSourceStart() + " error=" + problem.isError());
                }
                unit.accept(new ASTVisitor() {
                    /** 原始字符位置只用于证据，不用作最终节点身份。 */
                    @Override public void preVisit(ASTNode node) {
                        if ((node.getFlags() & (ASTNode.MALFORMED | ASTNode.RECOVERED)) != 0)
                            rows.add("RECOVERED_AST " + relative + " " + node.getStartPosition());
                    }
                    /** 只枚举源码中的显式方法与构造器。 */
                    @Override public boolean visit(MethodDeclaration node) {
                        rows.add("DECL " + relative + " " + node.getName()
                            + " " + binding(node.resolveBinding()));
                        return true;
                    }
                    /** 普通调用和声明绑定独立记录，观察重载是否正确。 */
                    @Override public boolean visit(MethodInvocation node) {
                        rows.add("CALL " + relative + " " + node.getName()
                            + " " + binding(node.resolveMethodBinding()));
                        return true;
                    }
                    /** new 的隐式构造器可能有 binding，却没有显式源码节点。 */
                    @Override public boolean visit(ClassInstanceCreation node) {
                        rows.add("NEW " + relative + " " + binding(node.resolveConstructorBinding()));
                        return true;
                    }
                    /** 方法引用不是立即执行的调用。 */
                    @Override public boolean visit(ExpressionMethodReference node) {
                        rows.add("REFERENCE " + relative + " " + binding(node.resolveMethodBinding()));
                        return true;
                    }
                    /** 类型形式的方法引用同样单独记录。 */
                    @Override public boolean visit(TypeMethodReference node) {
                        rows.add("REFERENCE " + relative + " " + binding(node.resolveMethodBinding()));
                        return true;
                    }
                });
            }
        }, null);
        Collections.sort(rows);
        for (String row : rows) System.out.println(row);
    }
}
'''
main = '''package p;
class Main {
    static int a = Helper.pick(1);
    static { Helper.pick("init"); }
    int run() {
        Runnable task = () -> Helper.pick(2);
        Runnable anonymous = new Runnable() { public void run() { Helper.pick(3); } };
        java.util.function.IntUnaryOperator reference = Helper::pick;
        return Helper.pick(4) + "中文😀".length();
    }
    void missing() { missing.Type.nope(); }
}
record Pair(int x, int y) { Pair { if (x < 0) throw new IllegalArgumentException(); } }
sealed interface Shape permits Circle {}
final class Circle implements Shape {}
'''
helper = '''package p;
class Helper {
    static int pick(int value) { return value; }
    static int pick(String value) { return value.length(); }
}
'''
with tempfile.TemporaryDirectory(prefix='narrant-java-probe-') as temporary:
    base = Path(temporary)
    bridge_file = base / 'Probe.java'
    bridge_file.write_text(bridge, encoding='utf-8')
    # 只编译 Narrant 探针自身；禁止 annotation processor。
    subprocess.run([str(javac), '-proc:none', '-encoding', 'UTF-8', '-cp', cp,
                    '-d', str(base), str(bridge_file)], check=True, env=env,
                   cwd=base, timeout=60)
    outputs = []
    for side in ('before', 'after'):
        root = base / side
        package = root / 'src' / 'p'
        package.mkdir(parents=True)
        (package / 'Main.java').write_text(main, encoding='utf-8')
        (package / 'Helper.java').write_text(helper, encoding='utf-8')
        command = [str(java), '-Xmx512m', '-cp', str(base) + os.pathsep + cp,
                   'Probe', str(root), 'src/p/Main.java']
        result = subprocess.run(command, capture_output=True, text=True,
                                check=True, env=env, cwd=base, timeout=60)
        outputs.append(result.stdout)
        print(side, result.stdout)
    assert outputs[0] == outputs[1], '观察值存在 checkout 路径或非确定性差异'
    command.append('src/p/Helper.java')
    all_selected = subprocess.run(command, capture_output=True, text=True,
                                  check=True, env=env, cwd=base, timeout=60)
    print('selected-helper', all_selected.stdout)
```

期望检查：`pick(int)`/`pick(String)` 的 declaration binding 区分；lambda/匿名类内调用能遍历；标准库 length 有可核实来源；缺失 Type 有诊断/null/recovered 证据；两个绝对目录输出可对应。选中 Helper 后增加其声明，但已有调用的 declaration identity 不变。probe 的 raw key 比较只是观察，不是批准它成为生产 NodeId；JSON/CodeGraph 校验和自己的 ID/diff 还须后续接入测试。Python timeout 不是完整的跨平台进程树回收实现，不能用此 smoke 代替 P3。

### P1–P3：决定能否进入实现的反例

| 实验 | 操作 | 必须记录/通过的条件 |
| --- | --- | --- |
| P1 目标 API 隔离 | 在 target=17 加 `java.util.List.of(1).getFirst()`，并在 8 加 `List.of(1)`；运行 VM 选 25 | 对照 javac `-proc:none --release 17/8`。JDT 单纯 source/compliance+bootclasspath 若接受这些 API，记录反例；改用真实 release/system API 配置后必须拒绝其确定合法绑定。JavaCore.COMPILER_RELEASE 是候选选项，不假定 standalone 环境必然正确使用它 |
| P1 泛型/重载 | primitive/boxing、varargs、null 歧义、泛型上界、协变返回/bridge；增加/移除仓库 fixture 中的上下文声明 | 源节点 ID 不因解析成功/失败改变；有证据的唯一声明才确定；歧义不取候选第一个 |
| P2 多项目/JPMS | 同仓库两个同 FQN 项目、共享文件、两个 named modules，变更 requires/exports/opens；添加 import module（若版本支持） | 非可读模块/非导出包不产生可访问的确定目标；项目间不串；不复制源码。如果 standalone JDT 无法表达，淘汰/调整后端或明确拒绝该配置 |
| P2 无构建副作用 | 仅在一次性 fixture 中给 Gradle settings/build、Maven import mapping/plugin、annotation processor 写 sentinel；只运行受控 Core 模式，不启用导入器 | 无 sentinel、无新增产物、无构建脚本/插件加载、无项目进程/网络；不能仅凭未看到文件就证明零网络或完整隔离 |
| P3 生命周期 | 在初始化、解析大文件、查询、输出阻塞期间取消；模拟异常退出/坏帧/缺 JAR | worker 返回前 JVM/所有后代/I/O 结束；before 删除成功、head 不变；两侧互相取消保留首个实错；Windows 和 macOS 都原生测 |

P1 的 `--release` 语义与 JDT 候选选项来自 [javac 手册](https://docs.oracle.com/en/java/javase/25/docs/specs/man/javac.html)、[JavaCore.COMPILER_RELEASE](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/JavaCore.html#COMPILER_RELEASE)。原研究的“增加/移除第三方 JAR”仍可作为独立引擎对照实验，但不属于当前生产输入验收。JDT 公开问题 #4912 可作为 P2 扩展 fixture 线索，不能当作本轮实测结果。

### 正式适配器验收矩阵

| 维度 | 最少样例 / 成对变化 | 通过标准 |
| --- | --- | --- |
| 对象覆盖 | package-info/module-info、class/interface/enum/annotation/record/sealed；局部/匿名类；字段/初始化块/构造器 | 六类映射完整；对象只存一次；空文件行为明确；无源码隐式成员不冒充 Node |
| 自身内容 | 只改子方法、字段 RHS、注解成员的 `default 1` 为 `default 2`、注释/空白、text block 内容、Unicode escape；交换字段/块/组件/枚举常量 | 子改动不污染父；注解默认值改变对应 Callable.signature；纯格式无 diff；语义值及初始化次序能比较；source 完整 |
| 身份 | 两绝对 checkout、选集扩缩、插入方法、增加重载、泛型参数改名、缺依赖恢复 | 稳定声明保持 ID；Node/External 切换不改符号身份；不做 rename/move 猜测 |
| 调用关系 | static/virtual/interface/abstract/default、this/super/new、method reference、lambda/匿名类、递归 | 静态声明与动态实现区分；引用不变调用；无外层伪 self-call；关系去重 |
| 生成代码 | record 默认/显式成员、默认构造器、Lombok、已提交 generated `.java`、陈旧 build 产物 | 显式源码不丢；不执行生成器；可确认生成目标 External，否则 Unresolved；旧产物不污染 Snapshot |
| 上下文 | Maven 多 module、Gradle 常规/自定义 source sets、缺 parent/BOM/JAR、Android SDK 缺失 | 明确支持范围；未知值不伪造配置；本地关系仍可用；四类目标分界准确 |
| 测试 | main/test 共用根、Test 后缀生产类、JUnit4/Jupiter/TestNG/JUnit3、元注解、缺框架 | false 只排除确证测试及其引用/内容；true 恢复原选集；未知/共享保留 |
| 输入/配置 | UTF-8、非 UTF-8 显式编码、BOM、CRLF、中文/非 BMP、外向链接、错误 target/preview、用户环境污染 | 首版建议 UTF-8，非兼容编码明确失败；坐标校验；配置/路径边界失效硬错 |
| 包与工具 | 两平台 PackLease、权限/缺文件/摘要/大小、重试/删除/历史版本 | 协议 smoke 和最小图成功；工具失败明确；工具/工作区租约完整 |
| 应用 | Rust+TS/JS+Python+Java、仅 Java 删除、后缀改语言、空侧、刷新/取消/失效结果 | 两侧独立路由；任一硬失败阻止完整比较成功；无悬空 Node；head 不写入 |
| UI（实施阶段） | Java 长类名/泛型、record/接口、语言切换、包下载失败/重试、宽窄窗口/展开 | Preview Harness + 应用内浏览器实测；现有选择与外部关系显示规则不回退 |
| 性能 | 冷/热 JVM、1/100/1000 文件、多根、缺依赖、两侧并发 | 报告耗时/RSS/查询量/缓存命中/取消延迟；据实测制定阈值，不虚报数据 |

以上未运行实验均为发布前待办。实施时 Rust 验证须经 `scripts/cargo-agent.sh`，私有测试放 tests/；前端还需单测/类型/Lint/生产构建与真实浏览器验收。本次纯文档调研不运行 Cargo、前端构建或 quality-gate。

## 分阶段建议与仍需决定的问题

1. **选型实验：** 用同一批小型 fixtures 对照 JDT Core 与 javac；先验证 target release、JPMS、重载/泛型、缺失依赖、Lombok 无生成模式与取消。实验通过才锁定具体 JDT/Temurin 发行版；否则在选型阶段优先改选 javac 重新验证。生产只采用选定后端，不同时运行两个分析器，也不做失败自动切换。
2. **有限范围可交付：** 独立 Java analyzer + 固定包，明确支持的 source set / Java version / 标准库范围；实现结构与静态可确定关系、自身内容、身份与四类目标；硬错误中止比较。
3. **扩大项目模型：** 支持仓库内多源码根与明确的静态配置，并验证明确目标标准库/平台声明的提供方式。目标声明进入分析指纹并保持两侧能力一致；不将任意现有第三方 JAR 作为后续默认增强。

**已排除路线：** Maven/Gradle/JDT LS 自动构建项目导入及运行构建脚本/插件不属于当前分阶段计划；只有用户明确改变决定后才重新评估。

除 ADR-0034 已确认的架构、执行与默认输入范围外，仍需决定的产品问题如下；表内推荐项**尚未视作用户已经批准**。它们不能由 JavaParser/JDT LS 默认配置代替决策，也不阻碍本次调研文档交付。

| 问题 | 推荐项 | 选择更广范围的代价 |
| --- | --- | --- |
| Java 版本/preview | 先验证 17/21/25；8/11 单列兼容门槛；preview 默认关闭 | 不是宣称仅支持这些版本；若要求全版本连续覆盖，扩大语法和标准 API 矩阵，固定包须同步兼容 |
| JPMS 是否首版必达 | 至少 module-info 内容可比较；完整多模块语义必须通过 P2 才承诺 | 若首版必须完整 JPMS，JDT Core P2 失败应在选型阶段改选 javac 重新验证，不能扁平 classpath 蒙混通过 |
| 局部/匿名/lambda 粒度 | 显式局部/匿名类独立；lambda 内联所属内容 | 全部独立更细但身份/图噪声更大；全部内联则类成员变化只表现为外层内容变化 |
| 隐式与生成成员 | 仅显式源码对象；已确认隐式目标可 External；不运行 Lombok/处理器 | 展示生成成员需来源/稳定身份/无真实 source 的界面规则，以及执行与快照归属设计 |
| 测试识别 | 确证 test source set/框架绑定；未知保留；不按短名 Test 自动过滤 | 想在依赖缺失时仍按命名排除，需要用户明确接受可误判的固定约定 |
| 用户项目 JDK | 工具 JDK 自包含；目标 API 可被固定 profile 满足时不依赖本机 | 接受用户 JDK/Android SDK 等意味着额外来源校验与可用性/重现性边界，但仍不自动安装工具链 |


## 本次验证的边界

本次完成：实际阅读现有三个适配器、公共图/身份/差异/生命周期契约、支持包模型/服务/存储/打包脚本，以及指定规格和 ADR；联网核对官方 API、第一方源码、构建工具文档及许可证；另有独立只读契约审查。文档本地链接均已检查，嵌入 Python 探针通过 `ast.parse` 语法检查；这不代表内嵌 Java 已编译。

本地只读工具探测结果：`java`/`javac` 指向 `/Users/lizhanfa/.jenv/shims/`，两个 `-version` 命令均报告 `jenv: version 17 is not installed`；`/usr/libexec/java_home -V` 未找到注册的 Java runtime。结论仅是本会话默认入口无法启动可用 JDK，不推断整台机器绝无其他 JDK。没有进行大型安装或冷编译。

没有安装 Java 后端、构建支持包、导入真实 Maven/Gradle 项目或运行 Java 对照实验；未测性能、内存、零网络访问、子进程回收、跨平台运行与完整再分发依赖清单。公开 issue 是实验线索，不是本机复现。未运行 Cargo/前端测试，未触发 quality-gate，未修改业务代码或其他语言研究文档，未创建 Issue/PR、提交或切换分支。后续实施不得引用本报告声称 Java 验收已通过。

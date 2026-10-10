# C/C++ 语言支持调研

后续工程草案：[公共进程协议](../specs/0024-native-analyzer-helper-protocol.md)、[语言配置与验收](../specs/0024a-native-language-analysis-profiles.md)。本文保留调研过程中的候选和历史待决项；已确认决策以 ADR-0034 为准，工程建议以草案和后续实测修正为准，未验收项不构成发布承诺。

共同输出语义已由 [ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 确认：helper 返回单张配置并集图，对象和已解析关系至少在一种已分析配置下成立，不保留配置归属，不新增配置切换/变体界面。必要的配置分析与兼容源码对象归并在 helper 内完成；无法正确归并按完整图契约失败。下文提到的配置结果封套、条件图或变体展示仅是未采用的研究候选，不能把并集理解为穷举宏组合或直接同时启用互斥源码。

调研日期：2026-10-02。仓库参考基线：`6761aca`。本文是研究与待决方案，不是已接受的 C/C++ 产品规格；“事实”指已读取的当前仓库、官方文档或一手源码，“推断”指由这些证据推出的约束，“建议”指待实施的方案，“未验证”指尚未完成实际实验或产品验收。

## 后续已决范围

[ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 已确定 Java、Go、C#、C/C++ 的接入边界：各自原生 helper 在同一进程中取得语法与语义，负责语言类别、内容投影、稳定身份、图成员及 Node / External / Unresolved 关系分类，通过统一接口返回规范化 `CodeGraph` 构造 DTO。Rust 负责输入来源边界、协议、通用校验、最终 `CodeGraph` 构造与生命周期；不跨进程传输编译器整棵 AST 再由 Rust 完成语言映射。C/C++ 共用 helper 核心的建议仍需分别验收 C 与 C++。

四门新语言均禁止执行被分析仓库及依赖携带的构建脚本、插件、注解处理器、生成器等代码；可以使用受控分析工具，但不能借项目加载触发上述执行。默认分析范围为仓库源码与明确目标的标准库/平台声明；不下载或 restore 第三方依赖，不自动纳入本机第三方缓存或环境，也不因业务依赖已经下载而允许读入。已提交的源码按项目与选集规则处理，不因位于 `vendor/` 一概排除。

Rust、TS/JS、Python 现状不变，Rust 可选 `all-dependencies` 保留。字段级协议、目标支持矩阵及具体映射仍待设计和验收；下文保留候选比较和已有探针事实，不表示原生 helper 或完整语言支持已经实现、验证。

## 1. 结论与语言边界

**对于“支持 C++ 是否就同步支持 C”：可以复用大部分 Clang/clangd 基础设施，但不能把 C 源码按 C++ 解析，也不能只验收 C++ 后宣布支持 C。** 建议交付一个 C/C++ Language Support Pack、一个共享实现核心，同时保留 `c` 与 `cpp` 两个语言身份、不同语言模式及各自验收矩阵。Clang 的 `-x` 选择输入语言，`-std` 选择语言标准；clangd 也需要按每个文件的编译命令判断 C 或 C++，编译器名称和参数都会影响解析。[Clang 语言模式](https://clang.llvm.org/docs/CommandGuide/clang.html#language-selection-and-mode-options)、[clangd 编译命令](https://clangd.llvm.org/design/compile-commands#what-can-be-in-a-compile-command)

事实：以下不是边缘的编译器 bug，而是语言规则差异；验收应显式指定 C11 与 C++17，避免默认标准和 GNU 扩展掩盖区别。标准证据为公开委员会草案，不声称是购买版 ISO 标准正文。

| 源码 | C11 | C++17 | 一手证据 |
| --- | --- | --- | --- |
| `void f(void *raw) { int *p = raw; (void)p; }` | 允许 `void *` 隐式转换为对象指针 | 不允许该隐式转换，需要显式转换 | [C N1570 §6.3.2.3](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1570.pdf#page=73)、[C++ N4659 Annex C.1.3](https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2017/n4659.pdf#page=1452) |
| `struct Item { int value; }; typedef int Item;` | 合法；tag 与普通标识符属于不同命名空间 | 非法；不能在该作用域把已有类名改作不同类型的 typedef | [C N1570 §6.2.3](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1570.pdf#page=55)、[C++ N4659 §10.1.3](https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2017/n4659.pdf#page=175) |
| `int n = sizeof('x');` | `'x'` 是 `int`，结果为 `sizeof(int)` | `'x'` 是 `char`，结果为 1 | [C N1570 §6.4.4.4](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1570.pdf#page=87)、[C++ N4659 Annex C.1.1](https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2017/n4659.pdf#page=1450) |

第三行在两种模式下都可能成功，但类型和语义不同；不能把结果一律写成“4 与 1”，因为 `sizeof(int)` 取决于目标平台。另一个容易误判的例子是变长数组：它属于 C99/C11 的语言功能范围，在 C11 中可选，不属于标准 C++；Clang 能作为 C++ 扩展接受它，因此兼容性探针必须包含 `-pedantic-errors` 或专门的 VLA 错误开关。[Clang VLA 兼容说明](https://clang.llvm.org/compatibility.html#variable-length-arrays)

建议：`.c`、`.cc/.cpp/.cxx` 是初始分类线索，显式编译命令优先；`.h` 不能固定解释为 C 或 C++。同一头文件可能被 C 和 C++ translation unit（下称 TU）分别包含，`__cplusplus` 分支、类型规则和链接规则随实际上下文变化。Clang 驱动源码确实区分 C、C++ 及头文件输入类型；clangd 对头文件则优先借用已打开包含者的命令，缺少包含者时会用文件名启发式推测，官方明确承认该推测可能错误。[Clang 驱动类型映射](https://github.com/llvm/llvm-project/blob/main/clang/lib/Driver/Types.cpp)、[clangd 头文件命令](https://clangd.llvm.org/design/compile-commands#commands-for-header-files)

`extern "C"` 只改变 C++ 声明的语言链接等规则，不把整个 C++ TU 切换为 C parser，也不能凭链接名断言某个 C++ 声明与另一侧 C 实现相同。Clang 对函数提供 `getLanguageLinkage()`、`isExternC()`、`isInExternCContext()`，说明“用什么语言解析”和“函数是什么链接约定”是不同信息。[Clang FunctionDecl API](https://clang.llvm.org/doxygen/classclang_1_1FunctionDecl.html)

## 2. 当前仓库对方案的约束

事实：现有公共接口是 `LanguageAnalyzer::analyze(&AnalysisInput) -> Result<CodeGraph, AnalysisError>`，输入只有本侧固定仓库根、选中文件和 `include_tests`。文件选择决定图成员，未选仓库源码仍可作为语义上下文。节点采用六类与语言原生 kind，`code` 比较自身内容，`source` 仅用于展示；节点和 External 身份不得包含 checkout 绝对前缀。[analyzer.rs](../../src-tauri/src/structure_review/code_graph/analyzer.rs)、[node.rs](../../src-tauri/src/structure_review/code_graph/node.rs)、[0018](../specs/0018-structure-review-code-graph.md)

已实际阅读的实现说明“同一个公共接口”不要求“同一种后台架构”：Rust 本地语法提取后用 rust-analyzer，TS/JS 自有相对路径身份，Python 先分项目再启动受控会话。这些现状与 Rust 可选依赖范围保持不变；C/C++ 默认依赖边界由后续 ADR-0034 明确，不是自动扩展 Python 的实现规则。[Rust adapter](../../src-tauri/src/structure_review/rust_analyzer/mod.rs)、[TS 身份](../../src-tauri/src/structure_review/typescript_analyzer/identity.rs)、[Python adapter](../../src-tauri/src/structure_review/python_analyzer/mod.rs)、[ADR-0033](../adr/0033-bound-python-analysis-to-repository-and-standard-library.md)

建议遵守以下现行决定，并单独处理尚未解决的冲突：

| 决定/契约 | C/C++ 接入影响 |
| --- | --- |
| [ADR-0030](../adr/0030-use-extensible-code-object-kinds.md) 六类与 native kind | 不新增 `Template`、`Macro` 等公共枚举；需要时采用原生 kind/扩展字段，但不能把 parser 节点原样当领域对象。 |
| [ADR-0031](../adr/0031-reuse-review-head-for-structure-analysis.md) 两侧固定工作区、无语言源码/配置副本、退出前回收子进程 | helper 与 clangd 直接读各自侧的 checkout；重定位 compile commands 在内存完成。不能复制源码树解决绝对路径，也不能让语言服务器越过租约继续读 before 目录。 |
| [ADR-0032](../adr/0032-separate-language-support-packs-from-project-toolchains.md) 固定支持包与项目工具链分离 | 固定 Clang 分析 helper/可选 clangd，不安装或替换用户 GCC、MSVC、SDK、CMake 或项目依赖。 |
| [ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 新语言 helper 与分析边界 | helper 内完成语法语义与语言映射，返回规范化构图 DTO；禁止执行项目/依赖代码，默认只读仓库源码与明确目标标准库/平台声明。 |
| 0018 的选中文件完整性 | **纯 Clang AST 只包含本次预处理实际活动的声明**，不等价于 Rust 现有语法层保留显式声明的范围。必须决定首版是配置限定图，还是另补原始语法层；不能悄悄把所有未活动对象当作不存在。 |
| 当前图没有多位置声明/配置维度 | C/C++ 同一实体有多处声明、跨 TU 和多种宏配置；不能仅因 USR 相同就合并所有源片段、丢掉所有权，也不能把多个配置的互斥事实无条件并入一个图。 |

最后两项不是已接受 ADR 的隐含例外。采用“一个明确构建配置下的活动声明完整图”需要写入新语言规格，并明确与 0018 的关系；要暴露部分/降级结果，还需决定在应用结果中如何表达完整性。现有成功 `CodeGraph` 没有表示“读取失败后只剩一半”的合法通道，硬错误不能成功空图。

## 3. AST 与语义后端候选

下表保留候选比较证据。后续已决路线要求原生 helper 内取得语法语义并完成语言映射；独立 clangd 与 Rust FFI/语法层候选不改变这一分工。

| 方案 | 官方能力与边界 | 对 Narrant 的评价 |
| --- | --- | --- |
| Clang LibTooling + AST/Sema | 可独立按选定文件运行 FrontendAction，完整访问 Clang AST；C++ API 不承诺稳定，需要随 LLVM 版本适配。[接口选择](https://clang.llvm.org/docs/Tooling.html)、[LibTooling](https://clang.llvm.org/docs/LibTooling.html) | **首选**固定版本原生 sidecar。一次 TU 解析同时得到声明、类型、重载解析、源码位置与预处理证据，适合自身内容和宏归属；维护一个小型 C++ helper 是明确成本。 |
| libclang C API | 稳定、高层、强调向后兼容的 C 接口；cursor 可遍历声明、拿类型/USR/位置，但官方明确不提供 Clang AST 的全部控制。[官方 tutorial](https://clang.llvm.org/docs/LibClang.html)、[接口选择](https://clang.llvm.org/docs/Tooling.html) | **备选**。Rust FFI 或原生 helper 容易接入，但需要先证明宏源码范围、模板、约束和完整自身内容需求均可表达；不能因为“有 AST cursor”就认为覆盖等价于 LibTooling。原生崩溃隔离仍宜在 sidecar。 |
| clangd | 内嵌 Clang driver/parser，提供 LSP 导航、符号、引用与索引；不是通用 AST 完整导出协议。[设计](https://clangd.llvm.org/design/)、[功能](https://clangd.llvm.org/features) | **研究对照/备选**，当前主链路不接入独立 clangd。其跨 TU 导航/索引可供比较；若与 AST helper 同时使用，会重复构建部分 TU，LSP 目标也不足以提取全部六类对象及自身内容。 |
| tree-sitter-c / tree-sitter-cpp | Tree-sitter 是增量语法树工具，面向语法与错误恢复；C 和 C++ 有不同 grammar，C++ grammar 在 C grammar 基础上扩展。[Tree-sitter](https://tree-sitter.github.io/tree-sitter/)、[C grammar](https://github.com/tree-sitter/tree-sitter-c)、[C++ grammar 源码](https://raw.githubusercontent.com/tree-sitter/tree-sitter-cpp/master/grammar.js) | **语法降级候选**。可观察原始 `#if` 分支和 token 范围；不运行 C 预处理语义、不做类型/重载/包含路径解析，宏生成声明可能不是完整合法语法树。不能用它把未知引用提升为确定关系。 |

推断：与 Rust/TS/Python 相比，C/C++ 的“纯语法层＋另外一个语义服务”更容易出现两层看到不同程序的问题。`typedef` 名称、宏替换、条件编译和模板实例化会影响解析，LibTooling 能把活动配置的语法和语义建立在同一 TU 上；代价是缺 include/SDK 时语法提取本身也可能受损。把 tree-sitter 与 clangd 组合起来并没有消除此问题，只是把它变成两套结果的对齐问题。

建议不把 Clang 插件作为首版入口：官方把它定位为编译过程中的 action，Narrant 当前需要独立分析选中文件；不应为了结构审查要求用户改真实构建命令或加载 Narrant 的编译器插件。[Clang 接口选择](https://clang.llvm.org/docs/Tooling.html)

## 4. 推荐架构与支持范围

建议采用 `CAndCppLanguageAnalyzer` 作为 Rust 侧深模块，落实输入来源边界并调用固定 LLVM 版本的 `narrant-clang-analyzer` 原生 helper。helper 按项目/TU/语言模式/编译配置在同一进程内取得语法语义，完成对象类别、内容投影、稳定身份、图成员和 Node / External / Unresolved 关系分类，返回统一接口的规范化 `CodeGraph` 构造 DTO。Rust 负责协议、来源与通用校验、最终图构造和生命周期，不接收整棵 Clang AST 再做语言映射。输入和诊断的字段级协议仍待设计；本研究未实现或验证 helper。

1. **活动 TU 主链路：** LibTooling `FrontendAction` + AST visitor + `PPCallbacks`；只做语法/语义分析，不生成目标文件、不链接、不运行项目程序。官方 `SyntaxOnlyAction` 和 `ClangTool` 已提供独立 TU 运行入口。[LibTooling](https://clang.llvm.org/docs/LibTooling.html#writing-a-standalone-tool)
2. **按需跨 TU：** 第一阶段用 helper 内 AST 已解析声明和选定上下文中的声明表建立关系，未能发现实现时保留已解析声明目标；更广定义覆盖可评估 helper 内受控索引。独立 clangd 保留为研究比较，不把主链路改成跨进程拼接语法语义。不能把“没有索引到定义”等价成“符号不存在”。
3. **结构降级：** 无编译数据库时可以提供明确语言模式与最小参数的独立文件尝试；只有满足所选支持范围的对象提取完整性，才返回普通成功图。若只能取得 tree-sitter 恢复树，应把它标成独立的有限结构模式，或报告分析上下文不足，不能静默切换然后与完整图当同质结果比较。
4. **两侧一致：** 同一工具版本、映射约定、目标配置选择政策用于两侧，但每侧读取自己的配置与源码。共享可执行文件与不可变 builtin headers，可以各有进程/缓存；不能共享可变 TU 或索引实例而串入另一侧内容。

**工具版本可以统一，但进程和 AST 不能直接共享。** clangd 内嵌 Clang，LibTooling helper 也能从同一个 LLVM tag 构建，配套 resource headers 可来自同一安装；这减小语义版本差异。两者没有“共享已解析 AST”的标准 LSP 接口，启动 helper 与 clangd 不会自动省掉重复解析。LibTooling C++ API 不稳定，所以应固定 tag、构建选项和工具摘要，而不是运行时链接用户任意 `libclang-cpp`。[clangd 编译命令实现说明](https://clangd.llvm.org/design/compile-commands)、[Clang API 选择](https://clang.llvm.org/docs/Tooling.html)

事实：clangd 自带的是与内嵌 Clang 配套的 builtin headers，其余系统头来自平台或项目工具链。**自带 clangd 不等于自带 macOS SDK、Windows SDK/MSVC STL、Linux sysroot 或交叉目标标准库。** 缺少这些上下文时必须区分“这个调用无法解析”和“这个 TU 的声明都无法可靠解析”。`--query-driver` 是执行实际编译器探测 include/target 的机制，需受控允许列表，不能默认执行数据库中的任意程序；探测也不会凭空提供 SDK。[clangd 系统头与 query-driver](https://clangd.llvm.org/guides/system-headers)

## 5. Clang 证据能支持什么，以及不能自动解决什么

### 5.1 活动预处理分支与宏

事实：Clang 有 `MacroDefined`、`MacroExpands`、`InclusionDirective`、`If/Elif/Ifdef` 和 `SourceRangeSkipped` 等回调，能保留活动预处理过程及跳过范围。`SourceRangeSkipped` 是范围记录，不是未活动分支的完整类型化 AST。[固定 LLVM 21.1.0 PPCallbacks 源码](https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-21.1.0/clang/include/clang/Lex/PPCallbacks.h)

例如 `#if FEATURE` 两侧声明不同类型或不同函数，单次 Clang 解析只为当前 `FEATURE` 值建立语义。建议首版选择一个明确配置并在分析元数据中记录；跨配置结果不能不加区分地合并。若产品要求所有显式分支都出现，需另行评估 tree-sitter 原始语法层，给未活动声明明确的有限语义状态；宏可拼接 token、生成声明或不成对包围语法结构，不能保证 grammar 对所有原始分支完整提取。这个范围决定应先于实施。

事实：`SourceManager` 分别提供 spelling location 与 expansion location，因此宏定义文本的位置和宏调用发生的位置可以区别。它还区分物理文件位置与受 `#line` 影响的 presumed location。[SourceManager](https://clang.llvm.org/doxygen/classclang_1_1SourceManager.html)

建议的源码归属规则：普通声明以本侧物理源码为准；宏生成声明必须记录定义位置、调用位置和产物之间的证据。选中调用点不意味着把未选头文件中的整段宏定义复制进调用方 `code`；选中宏定义也不意味着将所有 TU 展开产物复制成它的子节点。纯隐式编译器产物不生成源码对象。无法获得连续、可信源码片段时，不伪造 `source`，也不把展开文本当用户原文。

待决：首版是否把宏定义单独表示为 `Extension/macro`。宏不是普通 Callable，函数式宏的文本替换不能冒充 `code.calls`。无论是否建宏节点，都需要保留会改变对象提取的宏/条件编译信息，否则只改 `#define` 就可能使结构变化不可解释。建议先对对象边界宏给出明确限制和诊断，不能在未验证阶段声称全面支持宏元编程。

### 5.2 USR、稳定身份与重载

事实：Clang 提供为声明、类型和宏生成 USR 的接口；固定版本源码会根据 C/C++ 模式、函数链接约定、模板参数/实参、参数类型等编码函数，部分局部或匿名声明还会包含位置信息。这说明 USR 很适合作为同次分析中的语义实体证据，却不保证是 Narrant 所需的跨编辑稳定 NodeId。[LLVM 21.1.0 USR 接口](https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-21.1.0/clang/include/clang/Index/USRGeneration.h)、[USR 实现](https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-21.1.0/clang/lib/Index/USRGeneration.cpp)

建议区分两种内部身份：

- **源声明身份**用于 Code Object：语言、仓库相对路径、词法 owner、native kind/角色、声明名与必要的重载判别。不使用 AST 指针、字节偏移或行号作为通常身份。重载判别纳入参数类型、限定符和模板角色；签名改变可能保守地成为删除/新增，不能猜测是哪一个旧重载被改名或改参。
- **语义实体键**用于聚合证据：USR/规范声明链加上真实源归属与配置。它是 helper 内部资料，不直接等于 NodeId，也不以名字匹配所有同名函数。碰到 USR 生成失败、歧义或冲突，应拒绝唯一目标断言。

局部具名类型/声明如果独立建模，建议使用 owner 和同类同名声明序号；增加同名声明可能使后续身份变化，应明确记录保守边界。匿名 record、匿名 namespace、局部静态对象和宏生成同名声明必须有专门碰撞测试。两侧 checkout 路径变化、增加空行、改变所选文件子集均不得改变未变化对象的身份。

### 5.3 声明、定义与跨文件所有权

事实：`FunctionDecl` 有 `getCanonicalDecl()`、`getDefinition()`、`redecls()` 和声明/定义区分；这些 API 处理当前 AST 可见的声明链，并不使一个 TU 自动拥有另一个未读 `.cpp` 的函数体。[FunctionDecl API](https://clang.llvm.org/doxygen/classclang_1_1FunctionDecl.html)

建议首版保留显式源声明的文件所有权：头文件 prototype、源文件 definition、类内方法声明、类外方法实现是源片段，不因为语义实体相同就丢掉其签名变化。共享图只有单一声明树和一个 `source` 字符串，跨文件合并为一个节点需要另行决定哪个文件拥有节点、选择子集如何影响内容、默认实参和属性如何归属。稳妥起点是分别保留声明片段节点，内部再建立相同实体映射；是否需要公开“声明对应定义”关系，属于产品/词汇决定，不能偷偷复用 `code.member_of`。

目标选择先以编译器证据决定具体声明/定义，再按图成员转为 Node 或 External，不能为了让边显示在图里而优先任意已选节点。跨 TU 定义不可见时，已经确认的仓库内声明仍可为 Repository External；如果只知道名称、没有合格声明证据则为 Unresolved。头文件在多个 TU 中重复出现要按真实声明与配置去重，不能重复生成同一节点。

### 5.4 模板、实例化、重载调用与函数指针

事实：AST 提供主模板、特化、实例化等类别，`RecursiveASTVisitor::shouldVisitTemplateInstantiations()` 控制是否遍历实例化，不能假设默认 visitor 就返回所有实例。clangd 的后台索引特别处理模板实例化中的引用，官方同时说明其他部分出于性能禁用这些引用的全面收集。[RecursiveASTVisitor](https://clang.llvm.org/doxygen/classclang_1_1RecursiveASTVisitor.html)、[clangd BackgroundIndex](https://clangd.llvm.org/design/indexing#backgroundindex)

建议：主模板和用户显式写下的特化是源码对象；隐式实例化与隐式构造/析构函数不批量创建无源码节点。显式实例化声明是否单独建节点需约定，但不能复制实例化后的函数体充当源内容。对依赖模板参数且尚未实例化的调用，记录 Unresolved；具体实例中确认的目标，不无条件回填为主模板对所有实参唯一的目标。概念、requires、约束差异需要进入签名/自身内容，而不是仅保留打印后的实例类型。

事实：`CallExpr::getDirectCallee()` 可以返回直接函数声明，也可能没有直接函数目标。[CallExpr API](https://clang.llvm.org/doxygen/classclang_1_1CallExpr.html)

建议：直接调用和已完成重载决议可形成确定目标；取函数地址属于函数值引用。`p()` 中 `p` 是函数指针、成员函数指针或可变别名时，不能只按类型匹配推测调用者；没有数据流证明则 Unresolved。虚调用最多说明静态选中的声明，不代表枚举所有运行时 override；必须沿用当前关系语义，不承诺完整运行时调用图。`extern "C"` 的跨语言归并也需要声明证据与目标一致性，不能只依赖导出名。

### 5.5 C struct/typedef/union 与自身内容

事实：Clang AST 包括声明、语句、类型等多个层级，并保留接近书写形式的表达式信息；它不是仅供代码生成的简化树。[Clang AST 介绍](https://clang.llvm.org/docs/IntroductionToTheClangAST.html)

建议：`struct S`/`union U`、`typedef` 别名分别保留 native kind；`typedef struct S S;` 的 tag 与别名不按短名称合并，匿名 record 与 alias 的源码所有权也要明确。字段顺序、bit-field 宽度、对齐属性、限定符、初始化器、枚举值和 flexible array member 都可能影响语义，不能只采集字段名。C 的 tentative definition 与外部声明需区分原生角色，不应错误地为每个 `int x;` 合并/生成定义体。

自身内容建议使用原始 token/源码范围的规范化投影：保留字符串、raw string、预处理指令所需换行、token 边界及重要标点；独立子对象替换为稳定身份锚点。类字段排列是语义内容，不能因 `contains` 为集合而丢失。普通非语义注释/排版忽略，但不能用正则直接删除空白或 `//`。宏字符串化会折叠并保留参数内部空白：`STR(a+b)` 与 `STR(a + b)` 可能生成不同字符串；`##`、续行、`__LINE__`/`__FILE__` 也需要专门投影政策，不能笼统承诺任何空白编辑都无变化。[预处理字符串化规则](https://gcc.gnu.org/onlinedocs/cpp/Stringizing.html) AST pretty-printer 可以辅助签名，但不保证原始宏拼写、别名、显式写法都保留，不能直接把整棵 AST 打印串当 `code`。

普通函数体属于 Callable，不额外包装 ExecutionRegion；lambda 和局部类型是否独立建模是 C/C++ 的新粒度决定，不能把 Rust 的闭包或 Python 的函数内部声明规则直接继承过来。建议首版 lambda 留在最近的显式 owner 内容中，具名局部类型可独立提取，但需要针对捕获、局部重载和子内容隔离验收。

### 5.6 C++ modules

事实：Clang 标准 C++ modules 文档区分 named modules、header units 与 BMI；BMI 依赖语言选项与原始源码一致性，`clang-cl` 的 Clang BMI 与 MSVC `cl.exe` 的 IFC 也不兼容。当前 clangd 功能页仍把 C++20 modules 标为实验功能，由 `--experimental-modules-support` 启用。[Clang 标准 modules](https://clang.llvm.org/docs/StandardCPlusPlusModules.html#consistency-requirements)、[clang-cl 与 IFC](https://clang.llvm.org/docs/StandardCPlusPlusModules.html#clang-cl)、[clangd modules](https://clangd.llvm.org/features#experimental-c20-modules-support)

建议：首版把 modules 设为单独能力门槛，不能因为 Clang 支持 C++20 语法就宣称任意 CMake/MSVC/GCC module 项目可用。需要按侧生成或获取与固定 analyzer 兼容的模块上下文，不能复用 head 的 BMI 分析 before，也不能无条件消费用户当前构建目录的 IFC/PCM。Clang 的 `-fmodules-embed-all-files` 会把源文件嵌入 BMI；用它复制源码绕过原始路径要求，与 ADR-0031 的无源码副本原则存在冲突，不能作为默认修复。[模块源码一致性](https://clang.llvm.org/docs/StandardCPlusPlusModules.html#source-files-consistency)

未验证：本研究尚未构建 LibTooling helper，也未执行 C++ modules 的完整真实项目验收。本文引用的 LLVM 21.1.0 源码用于提供可复核 API/实现证据，不代表已为 Narrant 选定最终支持包版本；滚动 Doxygen 页面可能对应开发版本，实施时必须在最终固定 tag 复核。

## 6. 六类对象与现有关系的具体映射建议

以下是建议，不代表所有构造已进入首版承诺。首版应先选定第 13 节的配置范围和粒度，再用相同规则分析两侧。`native_kind` 保留原语言构造，`language` 使用 `c` 或 `cpp`；C++ linkage 为 C 的函数仍属于按 C++ 模式解析的源码。

| Code Object | C / C++ 映射 | 自身内容与边界 |
| --- | --- | --- |
| Container | 有对象的文件、C++ namespace；module unit 待模块能力阶段 | 文件用于组织，不重复比较全文件；namespace 的重开片段按源位置所有权分别保留。同一文件多个匿名/重开 namespace 需稳定序号，不能把跨文件 namespace 合成一个有多个父的节点。 |
| Type | C/C++ struct、union、enum；C++ class；typedef 与 using 类型别名；显式类模板/特化 | 别名与实体分别建模。模板参数、基类列表、访问控制、属性、约束与未独立成员属于自身内容。独立字段用有序身份锚点，字段排序修改必须可见；`typedef struct {…} T` 不得把同一段定义复制给两个 owner。 |
| Callable | 函数 prototype/definition、C++ 方法、构造/析构、运算符、转换函数、函数模板/显式特化 | 重载分别建模；`noexcept`、cv/ref 限定、默认参数、`=default`/`=delete`、`explicit` 等进入签名；函数体及构造初始化列表按明确归属比较。类外定义词法归源文件，`member_of` 表达所属类型。无用户源码的隐式方法不生节点。 |
| Value | 字段、bit-field、全局变量、静态数据成员、常量、枚举成员 | 类型/存储类/限定符/宽度进签名，初始化器进自身 code。`extern`、tentative definition 与真实 definition 保留角色；不把 C 全局声明照搬成 Python 的模块执行区域。普通自动局部变量建议留在函数体。 |
| ExecutionRegion | 首版可以不产生；可选的 C++ 文件初始化顺序投影 | C/C++ 没有一般脚本式顶层语句。全局对象初始化归 Value，不伪造一个运行时函数；若需检测同一 TU 中初始化顺序，额外区域只记录有序声明锚点，不重复 initializer。此表示需产品确认。 |
| Extension | 建议用于显式宏定义、对象位置宏调用；可选 `static_assert`、显式实例化声明或预处理区域 | 宏正文按预处理 token 规则比较，不能按 Callable 对待。`#include`、`#if`、`#pragma pack` 等保留在有序指令投影中，独立宏用锚点扣除；不把未知宏展开冒充真实函数。C++ concept 可用语言专属 Extension，需与用户确认展示。 |

多 declarator（如 `int a=1,b=2;`）应分别识别对象，公共类型和单个初始化器的比较归属必须确定；展示 `source` 可以覆盖共有声明，但不能将 `b` 的初始化器算进 `a.code`。C 匿名 union 的提升成员、匿名 record、C++ anonymous namespace、friend 声明、局部静态变量、GNU 嵌套函数/statement expression 是专门夹具，不以默认 visitor 行为替代产品决定。初期建议明确排除 Objective-C/Objective-C++、CUDA/HIP、MSVC C++/CLI；工具支持更多方言不自动扩大 Narrant 的承诺。

**当前仓库事实：** 关系是 `semantic: String`，并非一个统一封闭枚举。0018b 推荐表写 `code.inherits`，而现有 TS/Python 真正发出的是 `code.extends`；这是一处现行词汇差异，不能在调研中说它们已统一。字段类型关系实际使用 `code.type_use`，普通数据读写已省略。[0018b](../specs/0018b-code-relationships.md)、[TS OccurrenceKind](../../src-tauri/src/structure_review/typescript_analyzer/syntax.rs)、[Python 引用收集](../../src-tauri/src/structure_review/python_analyzer/references.rs)、[Rust 关系范围](../specs/0019-rust-language-analyzer.md#relationship-extraction-scope)

| 关系 | C/C++ 建议 | 不应伪造的情况 |
| --- | --- | --- |
| `contains` | 单一源码声明父节点 → 子节点；每个 ID 仅出现一次 | TU 多次包含同一 header 不能产生多个声明父；语义 owner 不必是词法 parent。 |
| `code.calls` | 实际调用 occurrence → 已确认函数/方法/构造声明；运算符调用按 AST 证据 | 函数指针参数、成员函数指针、依赖模板调用不能按函数类型或名称猜目标；虚调用只表达静态声明导航。 |
| `code.references` | 取地址、传递函数值 → 已确认 Callable | 普通变量读取、typedef 名称、宏调用不能全塞入此边。 |
| `code.member_of` | 方法/字段/类外成员定义 → 语义 owner Type | C struct 与同名 typedef 不能靠文本等同；不能拿这条边表示“声明对应定义”。 |
| `code.extends` 或 `code.inherits` | 派生 Type → 已解析直接基类；建议沿现有实现用 `code.extends`，实施规格明确与推荐词汇的关系 | C 没有类继承；C++ 抽象类仍是基类，不根据纯虚方法猜一个 `implements` 边。 |
| `code.type_use` | 先限定字段类型中已确认、图内保留的 Type，遍历 pointer/array/template argument 并保留 typedef 目标 | 不额外展开所有函数签名/局部变量类型边；primitive 不制造节点，缺上下文不按名称猜。需明确与 TS/Python 的外部目标政策是否统一。 |
| `code.applies_to` / `code.implements` | 没有合适源构造时不生成；显式模板实例化若另建节点可讨论 `applies_to` | 不把普通模板使用都看作 implementation，不把所有 C++ 类继承混成 implements。 |
| `code.imports` / 宏使用 | 首版建议保留 include/import 源内容；是否公开 module import 或 `cpp.uses_macro` 另定 | 头文件包含是文本预处理，不等价于目标符号 import；不得臆造 `calls`。 |
| `code.reads` / `code.writes` | 沿用现行精简范围，不生成 | 赋值仍改变所属 code；内部函数调用仍需遍历。 |

已知指针调用仍应记录一个 `Unresolved { name }` 的调用 occurrence；未知被调表达式不是“没有调用”。反之 `int x; use(x);` 不需要为了 `x` 的数据读取额外添加不确定关系。模板、重载和别名证据只在同一个有效 TU/configuration 内可信，多个上下文冲突必须保留不确定性。

另需明确求值上下文：`sizeof(f())`、`decltype(f())`、`noexcept(f())` 中有调用语法但不代表执行 `f`。建议首版不发其 `code.calls`；如保留函数值关系，需单独规定，不能默认遍历所有 CallExpr。显式构造和运算符调用可按源码证据纳入，隐式析构、转换函数、new/delete关联的隐式分配/释放调用先不承诺，加入时补源owner与目标资格规则。这些是待确认的提取政策，不是完整运行时行为分析。

## 7. 图成员、仓库上下文与身份落点

建议 helper 按下面的顺序完成语言映射：校验 occurrence → 验证目标声明/源码归属 → 算稳定源身份 → 最后查本次图成员，再返回已分类的 DTO。Rust 校验输入来源、协议与通用图不变量，不重新推断 C/C++ 身份或目标类别。仓库自己的文件不因未选择就成为 Dependency；也不能为扩大图内边而额外把目标文件自动加入选集。

| 结果 | 进入条件 | C/C++ 例子 |
| --- | --- | --- |
| Node | 目标源声明已确定，且是本侧选中文件中保留的对象 | 选中的 `.c` 函数调用选中头文件中有确定声明的 API。 |
| Repository External | 目标在该侧仓库内，但未选择/未独立建模；稳定身份成立 | 未改动 `include/api.h` 的 prototype；按规则不独立建模但身份可确认的局部对象另有明确政策。 |
| Dependency External | 目标在明确目标的仓库外 SDK/标准库/平台声明中，声明与稳定来源明确 | libc 头文件中的 `printf`；本机已有第三方业务头文件不因此纳入默认上下文。 |
| Unresolved | 只观测到引用，缺少合格唯一证据 | 缺 header、依赖模板表达式、间接调用、多配置目标冲突、仅有可疑 index symbol。 |

`ExternalSymbol { name, scope }` 只用于显示/范围，不参与 diff；真正关系身份由 `ReferenceTarget::External.symbol` 承载。当前 `CodeGraph::merge` 不会自动把跨语言 External 变成 Node，也拒绝重复 ID 和冲突描述。C 与 C++ 建议由同一个 helper 完成经过证实的 `extern "C"` 声明链归属和图成员分类，再由 Rust 组装一次最终图；不要期待现有通用 merge 自动完成跨语言桥接。[graph.rs](../../src-tauri/src/structure_review/code_graph/graph.rs)、[Python 的本侧 External→Node 收敛](../../src-tauri/src/structure_review/python_analyzer/mod.rs)

同一 `.h` 被两种语言使用时，还存在一个产品选择：按明确主 TU 模式建立一个图，还是保留 `c` 与 `cpp` 两套视图。后者的节点可用语言区分，但必须标明上下文；它不是两份独立源代码修改。首版建议选择明确主 TU，混合上下文留到后续配置视图，不能仅为满足现有语言筛选而复制节点。

**helper 内的稳定身份建议：**

- 源 NodeId 采用转义后的语言/相对文件/词法 owner/kind/name/必要 overload key。重载 key 基于参数类型和成员限定符/模板角色，不包含参数变量名、函数体、默认参数值等一般可变内容；普通非模板函数通常排除返回类型，但函数模板的依赖返回类型、转换函数目标类型等必须按语言身份规则参与判别。Clang USR 对函数模板确实编码返回类型，不能套普通函数规则。[固定 USR 实现](https://raw.githubusercontent.com/llvm/llvm-project/llvmorg-21.1.0/clang/lib/Index/USRGeneration.cpp#L261) 模板 requires 改动和其他同名判别必须按固定版本规则验收。USR 作为验证证据；不承诺改参数仍保留同一个 ID。
- 同一语义实体多个源码声明分别保留源对象，内部建立 `entity → declaration/definition fragments` 表。同文件重复 `void f(int); void f(int); void f(int){}` 还需“声明/定义角色＋同角色片段序号”避免碰撞；插入同类重复声明可能保守改变后续身份，不猜测匹配。优先定义还是声明要由固定导航政策和已知上下文决定，不能因用户勾选文件不同而改变目标偏好。若两个配置给出不一致声明/定义，不取第一项。
- 外部身份使用确定的依赖来源 ID、目标/ABI、该来源内相对路径和声明键；不要保存用户 SDK 安装绝对目录或 hash 后的绝对目录。系统依赖位置可变而来源无法确认时，应使用保守身份/Unresolved，不能以裸 `printf` 名称假装可跨平台比较。
- 明确区分工具缓存键和语义 NodeId：snapshot、Clang 版本、配置摘要属于缓存；不应把每次 snapshot ID 加进所有 NodeId，否则两侧所有对象都成为增删。配置改变使声明含义改变时，比较前需验证两图处于可比较的目标政策。
- 宏 expansion/source range、UTF-8 字节、LSP UTF-16 位置基于同一原文校验；不得使用 `#line` 的虚拟文件名认定源码所属。非 BMP、CRLF、非 UTF-8 输入应有显式政策。初版建议 UTF-8，其他编码报不支持，不能按有损解码取得“成功”。

第三方“vendored 到仓库”的已提交源码按共享 `SymbolScope` 属于 Repository，按项目与选集规则处理，不因目录名 `vendor/` 一概排除或变成 Dependency。生成头若真实受 Git 管理且在该侧存在，可以按普通源文件处理；未跟踪构建目录生成物没有天然 Snapshot 保证，也不会仅因本机已存在而进入默认上下文。符号链接/junction/虚拟文件映射需要 canonical path 和来源校验，不能以 URI 字符串前缀判断。不运行项目生成器，不拷贝或跨侧复用生成物。

## 8. 编译配置、构建系统与降级

### 8.1 以编译命令而非项目文件名作为语义输入

事实：JSON Compilation Database 的一项包含 `directory`、`file`、`arguments` 或 `command`，可含 `output`；同一文件允许多条命令。`arguments` 避免 shell escaping 歧义，但命令本身仍可能含 response file、wrapper 或外部路径。[官方格式](https://clang.llvm.org/docs/JSONCompilationDatabase.html#format)

建议每侧构造内存 CompilationDatabase；适配器处理的是编译参数而不是执行数据库字符串。保留 `-x`/`-std`、GNU/MSVC 方言、`--target`、`-D/-U`、include 次序、sysroot、forced include、PCH/modules 和工作目录的语义。不运行 shell，不执行原 command，不加载 compiler plugin。未知参数不能全部悄悄删除；只删除确知与输出/codegen 相关的参数，不能删除会改变预处理/AST 的选项来凑一个成功结果。

| 构建系统 | 可用事实/输入 | 首版建议与缺口 |
| --- | --- | --- |
| CMake | `CMAKE_EXPORT_COMPILE_COMMANDS` 可在生成阶段导出；目前仅 Makefile/Ninja generators 实现，UNITY_BUILD 有限制。[CMake](https://cmake.org/cmake/help/latest/variable/CMAKE_EXPORT_COMPILE_COMMANDS.html) | 读取已有数据库；不假设 `.sln`、Xcode 工程或 `CMakeLists.txt` 本身已提供每 TU 参数。配置/构建可能生成头文件，首版不自动运行 configure。 |
| Make/自定义脚本 | 可由 Bear 拦截编译生成数据库，实际需要跑构建。[clangd 项目配置](https://clangd.llvm.org/installation.html#project-setup) | 文档指导用户生成，Review 分析不执行 `make clean`、项目脚本或依赖安装。若只有编译命令记录，仍验证来源和本侧路径。 |
| Meson | Ninja backend 使用 `ninja -t compdb`，支持时展开 response files 后输出数据库。[Meson Ninja 源码](https://github.com/mesonbuild/meson/blob/master/mesonbuild/backend/ninjabackend.py) | 消费构建目录中的已有数据库；cross-file/target 和生成头不能丢。其他 backend 需各自验证。 |
| Bazel | Clang 文档列出提取器；Hedron extractor 要求匹配 Bazel flags，支持目标范围及外部文件选项。[Clang 支持系统](https://clang.llvm.org/docs/JSONCompilationDatabase.html#supported-systems)、[提取器一手说明](https://github.com/hedronvision/bazel-compile-commands-extractor) | execution root、external repositories、generated headers 需要来源映射；不能将执行根副本当固定仓库源码，也不自动执行 Bazel 生成/下载。 |
| MSVC/Visual Studio | 可用 Ninja 生成 MSVC 命令数据库；工具初始化可能依赖开发者环境。[Clang Windows 配置](https://clang.llvm.org/docs/HowToSetupToolingForLLVM.html#setup-clang-tooling-using-cmake-on-windows) | 支持 `clang-cl`/CL 参数需验证 `/I`、`/D`、`/TC`、`/TP`、`/std`、response file、target 和 SDK；`.vcxproj`/`.sln` 不能直接当 CDB。Clang 的 MSVC 兼容性不是所有扩展完全等价。[兼容边界](https://clang.llvm.org/docs/MSVCCompatibility.html) |
| 小项目/无构建系统 | 可手工提供统一 `compile_flags.txt`；clangd 无 CDB 有简易 fallback。[clangd fallback](https://clangd.llvm.org/design/compile-commands#fallback-commands) | 统一 flags 适合模式一致的小项目，不足以表达多个 target/configuration。`.h` 无明确语言模式时不猜。 |

只读导入已有数据库仍需明确配置来源与本侧映射：仓库中跟踪的配置或来源可验证的已有命令数据；不能为取得数据库而运行被分析项目或依赖的构建脚本/生成器，也不能据数据库路径放宽默认第三方范围。当前 `AnalysisInput` 没有 C/C++ 配置选择字段，字段协议仍待设计，不能把外部路径偷偷塞进 `include_tests` 或 Rust 的 `RustDependencyScope`。

### 8.2 多项目、头文件和两侧路径

建议每个选中 `.c/.cpp` 根据本侧项目归属和明确目标选择一个命令，多个匹配命令先报告配置歧义；不得默认拿第一条。选择 Debug/Release、host/cross、feature variant 的政策两侧相同，但各侧的真实 `-D` 或标准版本可以随源码配置改变。目标 SDK 的替换不能与代码变化混在一次图比较里而无说明。

头文件通常没有独立命令，clangd 的包含者缓存和文件名相似度是导航启发式，不是确定 TU 归属的证据。[头文件策略](https://clangd.llvm.org/design/compile-commands#commands-for-header-files) 建议优先已验证的包含关系，记录采用的 TU；非 self-contained header 从包含它的 TU 中提取，按 header 的物理源范围筛出选中对象。多个 TU 结果完全等价才去重；若宏定义导致不同字段、签名或目标，首版要求选择上下文，后续才能按配置展示。没有相关 TU 的孤立 header，可在已明确模式下尝试独立解析；失败必须解释缺上下文。

数据库往往带原主工作树或旧 build tree 的绝对路径。重映射只允许证明为本仓库源码/配置的路径，且映射到本侧同一相对文件。逐项处理 `directory`、source、`-I/-isystem/-include`、`@rsp` 内部路径、sysroot、PCH/BMI；不能全字符串替换路径前缀就宣称正确。外部 SDK 不应该映射进仓库；Bazel execution root、PCH 和生成头不能凭文件名对应。若 before 缺该头或配置，不能向 head 借读。

LibTooling 可直接使用内存 CompilationDatabase。clangd 有 `initializationOptions.compilationDatabaseChanges`/配置变更扩展，按真实文件路径传 `workingDirectory` 与 argv，可用于无复制注入；但扩展可变，是否阻止所有磁盘数据库/父配置发现、覆盖 background index 需要专门原型验证。不能为了省事在语言临时目录复制一份清洗后的项目配置。[clangd 命令扩展](https://clangd.llvm.org/extensions#compilation-commands)、[ADR-0031](../adr/0031-reuse-review-head-for-structure-analysis.md)

clangd 默认会读取用户配置与祖先 `.clangd`，且用户配置优先。[配置发现](https://clangd.llvm.org/config#files) 建议关闭不受控发现，显式解析本侧仓库内允许的配置到内存，固定缓存/环境和编译器识别。已有 Workspace Trust 不放宽禁止项目/依赖代码执行的边界；不运行这些来源的 compiler wrappers。query-driver 仅是保留的候选工具能力，若后续采用，必须限于明确受控的分析工具并验证不会加载项目/依赖插件或脚本，也不因探测到本机第三方 include 目录而将其纳入。[query-driver](https://clangd.llvm.org/design/compile-commands#query-driver)

### 8.3 缺数据库、SDK、第三方时的可用性

| 条件 | 应输出的结果 |
| --- | --- |
| 无 CDB，但语言/标准/目标明确，独立源码能完整解析 | 可按显式独立文件 profile 产图，标明采用的分析上下文；不能声称覆盖真实项目构建。此 profile 与完整项目 profile 不应在两侧无提示混用。 |
| 第三方实现不可见，但已包含完整 prototype | 可以确认调用到该声明，目标是 External/Node；缺函数体不等于符号未解析。 |
| 关系缺少唯一语义证据，但承诺范围的对象仍完整 | 保留对象并标 Unresolved；不要仅按名字或 `definition` 非空就确定目标。 |
| 缺 header/SDK 导致 AST 恢复、类型/声明遗漏 | 完整模式失败。可以展示诊断；若另有经过验收的结构模式，需用户可见地选择，不能把半张 AST 当正常图。 |
| 工具包缺 resource headers/二进制，协议 EOF/超时，路径错侧，非法源码，helper 崩溃 | 硬失败并回收；不可返回成功空图、空关系或自动换另一套工具“补齐”。 |
| 选集为空/真正空文件/确定被测试规则排除 | 可以合法空图；先完成输入路径/读取校验。 |

事实：Clang 内部文档明确错误可能使 AST 不完整，找不到 include 还可能属于 fatal；本地第 11 节也实测了“退出非零但仍输出 AST”。因此“clangd 能打开文件”或“AST dump 是 JSON”不是完整性验收。[Clang diagnostics](https://clang.llvm.org/docs/InternalsManual.html#the-diagnostics-subsystem)

## 9. 固定支持包、平台工具链与再分发

### 9.1 包组成与版本政策

**事实：** 查询时 clangd 官方发布页列出稳定 `23.1.0`，说明基于 LLVM commit `ea7d852`；这可作为新实现的候选固定上游版本，不代表本研究已经验证它。本文实际运行的是本机 Apple Clang/clangd 21.0.0，两者证据分开。[clangd 23.1.0 发布](https://github.com/clangd/clangd/releases/tag/23.1.0)、[发布资产与摘要](https://github.com/clangd/clangd/releases/expanded_assets/23.1.0)

建议 C/C++ 共用独立版本包 `c-cpp`，固定完整 LLVM commit、helper 协议版本、构建参数、每个平台产物 SHA-256。首版只需要 helper 时不必强制下载 clangd；若采用 clangd 增强，则与 helper 同源构建并随包一起验证，不混用系统任意版本。LibTooling 是原生编译器库服务，不要求运行时安装 Python、Node、Java、CMake 或 LLVM 开发 headers；构建 helper 的 C++ 编译工具属于发布流水线，不是用户项目依赖。

| 建议包内内容 | 目的 |
| --- | --- |
| `bin/narrant-clang-analyzer[.exe]`，可选 `bin/clangd[.exe]` | 固定原生能力与进程协议；优先静态链接或打包精确原生运行依赖，避免用户 PATH 决定加载版本。 |
| `runtime/clang/<version>/include/…` | 与解析器匹配的 Clang resource headers；helper 显式 `-resource-dir`，clangd 显式 `--resource-dir`。不是完整平台标准库。 |
| 实际需要的原生动态库/运行时、工具元数据 | 按 macOS loader/Windows DLL 搜索规则打包并验证；不能只在开发机成功即称自包含。 |
| `licenses/…`，固定构建来源与哈希清单 | LLVM 许可和实际依赖归属；如有 tree-sitter，还需其 runtime/C/C++ grammar 许可。 |
| 必要 `control/…` | 仅 Narrant 自有控制数据；不存复制出来的项目配置或头文件。 |

**当前实现约束：** 现有 manifest 只允许 `bin/`、`runtime/`、`licenses/`、`control/`，不能直接加入上游 `lib/clang` 而不调整校验；建议上面的 resource-dir 布局。归档上限为 256 MiB，解压上限 1 GiB，文件数最多 20,000，所有产物必须是普通文件；是否放得下 helper＋clangd 需实测，不能将完整 LLVM 安装目录盲目打包。[storage.rs](../../src-tauri/src/language_support/storage.rs)、[打包脚本](../../scripts/build-language-support-packs.mjs)

当前 Narrant 支持包目标只有 macOS ARM64 与 Windows x64；Linux、macOS x64、Windows ARM64 是未来平台扩展，不能从 LLVM 支持它们推导 Narrant 已支持。[当前 model](../../src-tauri/src/language_support/model.rs)、[包服务平台选择](../../src-tauri/src/language_support/mod.rs)、[版本目录](../../resources/language-support-packs.json) 上游 clangd 构建脚本有 macOS universal、Windows runtime 与 Linux兼容性安排，但自建 helper 的最低 OS、动态依赖、签名/启动、中文路径和退出必须分别原生验收。[上游分发构建](https://github.com/clangd/clangd/blob/master/.github/workflows/autobuild.yaml)

### 9.2 支持包不提供的项目能力

Clang resource headers 如内建类型/内建函数头，与 libc、libc++/libstdc++、MSVC STL、Windows SDK、Apple SDK、Linux sysroot 是不同层次。Clang 官方工具链文档把 C/C++ 标准库及运行时作为独立组成；clangd 也明确依赖系统头来源。[Clang toolchain](https://clang.llvm.org/docs/Toolchain.html)、[clangd 系统头](https://clangd.llvm.org/guides/system-headers)

默认范围已确定为仓库源码与**明确目标**的标准库/平台声明；可评估已安装且来源可验证的匹配 SDK/标准库 profile，具体目标矩阵仍未确定，需记录 target/sysroot/include 来源，不安装、更新或覆盖它们。不假定安装 clangd 后 macOS 就能解析 Windows APIs；`--target` 只选目标，并不会下载相应 SDK。不下载/restore 第三方依赖，也不自动读入本机已下载的业务依赖、第三方缓存或环境 include；已提交源码仍按项目/选集规则处理。这一边界可能导致更多 TU 缺少必要头文件，必须如实失败或按已验收模式处理，不能暗中放宽范围。

外部 SDK 在两侧同时使用相同 profile 仍可能被其他进程更新。建议按实际读取的外部头/response file/模块依赖建立指纹，在结果完成前校验所需一致性；来源无法冻结或验证时报告上下文改变。ADR-0031 的 head Git 校验并不保护仓库外 SDK，不能宣称获得操作系统级快照隔离。

### 9.3 许可证与证据边界

事实：LLVM 当前主要采用 Apache-2.0 WITH LLVM-exception，官方政策允许包含在商业分发中，也提醒仍有历史许可组件。应随实际发行保留许可证及适用 NOTICE/归属，而不能只在文档写“LLVM 开源”。[LLVM 许可政策](https://llvm.org/docs/DeveloperPolicy.html#license)、[LLVM LICENSE.TXT](https://github.com/llvm/llvm-project/blob/main/llvm/LICENSE.TXT)

Tree-sitter runtime、C grammar、C++ grammar 各自提供 MIT 许可，若选该备选应分别归档固定版本许可证。[runtime](https://github.com/tree-sitter/tree-sitter/blob/master/LICENSE)、[C grammar](https://github.com/tree-sitter/tree-sitter-c/blob/master/LICENSE)、[C++ grammar](https://github.com/tree-sitter/tree-sitter-cpp/blob/master/LICENSE) SDK、MSVC redistributable、第三方库不是 LLVM 许可自动覆盖的对象；本方案不再分发用户 SDK。是否额外分发 libc++ 或某个 sysroot 需要另列具体产物与许可证据，当前未验证。实际包发布前还需核对所有动态依赖，本文不将源码许可等同于完成发行审核。

## 10. 生命周期、性能与缓存

事实：当前 application 对 before/head 并行，在每侧运行现有语言组，失败会撤销同组/对侧并 join；`PackLease` 由实际 worker 保持。共享 LSP 在 Unix 建立进程组，在 Windows 建立 Job，并有等待、强制结束及后代回收逻辑；只是 `shutdown` 返回不等于所有进程已退出。[application/mod.rs](../../src-tauri/src/structure_review/application/mod.rs)、[LSP transport](../../src-tauri/src/lsp/transport.rs)、[Windows Job](../../src-tauri/src/lsp/windows_process.rs)

建议每侧采用有界 TU worker 池，原型先设每侧 1 个 helper/TU worker，记录峰值后再调，避免现有两侧×多语言再乘上 clangd 的默认线程数。取消检查覆盖路径准备、TU 调度、AST遍历、关系转换与进程等待；LibTooling 内部长解析不能保证随时可中断，因此父进程必须有总截止时间和进程组/Job 回收兜底。helper 协议错误、超时、panic、取消均不返回部分图；清理失败须与原始错误一起可见。若 helper 不说 LSP，不能假装现有 `LspClient` 可直接接它，需要提取可复用的进程所有权实现或提供同等守卫。

clangd 的动态索引覆盖已打开文件及包含头；background index 会按数据库遍历整个项目 TU，模板实例化引用也可进入其中。缓存默认写数据库旁 `.cache/clangd/index` 或用户缓存；超大仓库可需要小时级索引与 GB 级内存，这不是本研究测得的 Narrant 数值。[clangd index 设计](https://clangd.llvm.org/design/indexing)

建议默认关闭 background index 和远程 index，只解析选集所需的已验证 TU/include 上下文。选择范围不等于完全不读其他仓库文件；但不能因此默许扫描用户主工作树或不受控依赖。开启后台索引必须显式限制该侧数据库、线程、内存预算、缓存路径与清理。`Index.Background: Skip` 按 TU 生效，并不能让某个被 include 的 header 从所有 AST 消失。[配置语义](https://clangd.llvm.org/config#background) `-j` 同时影响异步/后台索引工作线程，需结合两侧并行设置。[参数源码](https://clang.llvm.org/extra/doxygen/ClangdMain_8cpp_source.html)

缓存建议分三层：固定包只读文件；单次侧会话的源码/声明/定义查询缓存；可选 TU/preamble/index 缓存。后者键至少含工具及协议/映射版本、snapshot/侧、真实 source/包含依赖指纹、编译参数、语言模式、target/SDK、预处理配置、测试选项。头文件变化必须使所有依赖 TU 失效；不能仅按主 `.cpp` mtime 命中。相同二进制可共享，before/head 的可变索引不能共享目录；PCM/PCH 必须满足生成器版本和配置一致性，未知来源不可复用。暂时禁止跨次持久缓存比错误复用安全，也更容易验证。

性能验收记录冷/热总耗时、TU 数、重复 header 解析、AST/RSS 峰值、CPU、磁盘缓存、LSP query 数、取消至进程树消失的时间。当前没有大型仓库测量，不承诺相对 Rust/TS/Python 更快；加入 clangd 前要量化重复解析代价。应用关闭/refresh 后不得仍写 cache 或读取已回收的 before 目录。

## 11. 本轮实际探针与可执行的最小实验

### 11.1 已执行结果

2026-10-02 在当前 macOS ARM64 宿主运行 `/usr/bin/clang` 与 `/usr/bin/clangd`，二者输出 `Apple … version 21.0.0 (clang-2100.3.34.2)`，target 为 `arm64-apple-darwin27.0.0`。使用 Python 标准库在临时目录生成小夹具，未复制仓库源码，未编译 Narrant/LibTooling helper，未链接或运行被分析的 C/C++ 程序。所有临时目录已由 `TemporaryDirectory` 清理；这里的 Python 只是探针驱动，不是建议给产品引入的运行时。

| 探针 | 实际操作 | 观察到的结果 |
| --- | --- | --- |
| C/C++ 转换差异 | 同一 `void f(void *p) { int *q=p; (void)q; }` 使用 `-x c -std=c17` 与 `-x c++ -std=c++20`，都加 `-pedantic-errors -fsyntax-only` | C exit 0；C++ exit 1，诊断不能从 `void *` 初始化 `int *`。 |
| 关键字差异 | `int class = 1;` 同样分别解析 | C exit 0；C++ exit 1。 |
| 字符常量语义 | C `_Static_assert(sizeof('x') == sizeof(int), …)`；C++ `static_assert(sizeof('x') == sizeof(char), …)` | 各自对应模式均 exit 0；本例不依赖 `sizeof(int)==4`。 |
| 条件与宏 AST | `.c` include `api.h`；头中 `#ifdef FEATURE` 声明 active/alternate，另有 `DECL(generated)`；分别有/无 `-DFEATURE` 执行 JSON AST dump | 无宏时只出现 alternate，有宏时只出现 active；generated 的位置包含 spellingLoc/expansionLoc。 |
| 缺头不等于完整 AST | `#include "not_present.h"` 后接一个函数，执行 AST dump | exit 1、fatal file-not-found，但 stdout 仍有 AST JSON。 |
| 无数据库 fallback | 同一简单夹具执行 `clangd --check=… --background-index=false --enable-config=false` | 日志有 `Failed to find compilation database`、`Generic fallback command`；该夹具最终 0 errors/exit 0。只证明玩具例可用。 |
| 真实 LSP 重载 | CDB 为 C++20；`int pick(int)` 与 `double pick(double)`；查 `pick(1)` | definition 返回 int 重载的源声明范围。 |
| 真实 LSP 头文件声明 | `api.h` 内 `extern "C" int c_api(int);`；查 `c_api(2)` | 返回 `api.h` 的 prototype；没有返回不存在的 C 实现。 |
| 真实 LSP 间接调用反例 | `int use(int (*fp)(int)) { … fp(3); }`；查 `fp(3)` | definition 返回 `fp` 参数声明范围，不能映射为 Callable 调用目标。 |
| 正常关闭 | 同一会话 initialize/initialized/didOpen/definition/shutdown/exit | 三个 definition 请求无 JSON-RPC 错误；服务器 exit 0，读取线程已 join。未验证取消、崩溃、子孙进程和 Windows。 |

这些结果只证明列出的现象。它们**不证明**固定 23.1.0 包、完整映射、USR 跨 checkout 稳定性、头文件全配置、缺 SDK 降级、modules、性能或产品集成已通过。第 1 节的 typedef/tag 例子本轮依据标准草案核实，未另运行该例。没有运行 Cargo、quality-gate 或大型安装。

### 11.2 可直接复现的 CLI 最小实验

以下脚本在已有 Clang/clangd 与 Python 3 的开发机运行；不安装工具，仅做语法分析。它复现主要 CLI 现象，严格断言成功/失败与活动声明；实际探针的 LSP 部分另见下一段步骤。

```sh
python3 - <<'PY'
import json, pathlib, shutil, subprocess, tempfile

clang = shutil.which('clang')
clangd = shutil.which('clangd')
assert clang and clangd, '需要已有 clang 和 clangd，不由此脚本安装'
print(subprocess.check_output([clang, '--version'], text=True).splitlines()[0])
print(subprocess.check_output([clangd, '--version'], text=True).splitlines()[0])
with tempfile.TemporaryDirectory(prefix='narrant-c-cpp-') as directory:
    root = pathlib.Path(directory).resolve()
    sample = root / 'probe.c'
    sample.write_text('void f(void *p) { int *q=p; (void)q; }\n')
    for language, standard, expected in [('c', 'c17', 0), ('c++', 'c++20', 1)]:
        result = subprocess.run([clang, '-x', language, '-std='+standard,
                                 '-pedantic-errors', '-fsyntax-only', str(sample)],
                                capture_output=True, text=True, timeout=15)
        assert (result.returncode != 0) == bool(expected), result.stderr
        print(standard, 'exit', result.returncode)
    (root / 'api.h').write_text(
        '#ifdef FEATURE\nint active(void);\n#else\nint alternate(void);\n#endif\n'
        '#define DECL(name) int name(void) { return 1; }\nDECL(generated)\n')
    sample.write_text('#include "api.h"\nint use(void) { return generated(); }\n')
    for flags, present, absent in [([], 'alternate', 'active'),
                                   (['-DFEATURE'], 'active', 'alternate')]:
        result = subprocess.run([clang, '-x', 'c', '-std=c17', '-fsyntax-only',
                                 '-Xclang', '-ast-dump=json', *flags, str(sample)],
                                capture_output=True, text=True, timeout=15)
        assert result.returncode == 0, result.stderr
        declarations = [node for node in json.loads(result.stdout)['inner']
                        if node['kind'] == 'FunctionDecl']
        names = {node['name'] for node in declarations}
        assert present in names and absent not in names, names
        generated = next(node for node in declarations if node['name']=='generated')
        assert 'expansionLoc' in generated['loc'], generated
        print(flags, sorted(names), generated['loc'])
    checked = subprocess.run([clangd, '--check='+str(sample), '--background-index=false',
                              '--enable-config=false'], capture_output=True,
                             text=True, timeout=30)
    assert checked.returncode == 0, checked.stderr
    print('clangd fallback exit', checked.returncode)
    sample.write_text('#include "not_present.h"\nint surviving(void) { return 0; }\n')
    broken = subprocess.run([clang, '-x', 'c', '-std=c17', '-fsyntax-only',
                             '-Xclang', '-ast-dump=json', str(sample)],
                            capture_output=True, text=True, timeout=15)
    assert broken.returncode != 0 and broken.stdout, broken.stderr
    print('missing header: failed with AST output')
PY
```

AST dump 在这里用于开发探针，不作为正式产品协议。执行环境不同仍须记录工具版本和诊断；失败不应修改断言“迎合”结果。

### 11.3 下一轮原型的具体输入与验收矩阵

第一轮 LibTooling spike 只需 `FrontendAction + ASTConsumer + PPCallbacks` 输出自有 JSON；不要先做完整 UI。用两个临时目录 `before/head` 放相同相对夹具，每侧有各自的内存编译命令，验证 NodeId/External ID 无绝对根。下表中“未执行”明确指还没有运行端到端的 Narrant 验收，不是官方能力不存在。

| 维度 | 最小可执行输入/动作 | 验收要求 | 本轮范围 |
| --- | --- | --- | --- |
| C 语言模式 | 同一 `.h` 从 `-xc -std=c11/c17` 与 `-xc++ -std=c++17/c++20` TU include；加入 `_Generic`、designated initializer、typedef/tag、`__cplusplus` | 模式可追溯，两个语言独立测试；不把扩展接受等同标准兼容。C99/C23、C++23 另跑正式语言支持矩阵。 | 部分 CLI 现象已跑；全矩阵未执行。 |
| 六类与内容 | C struct/union/typedef、bit-field、enum、全局与局部变量；C++ namespace/class/method/template、`=default`、lambda | 精确 expected 图；方法体改动仅改变方法；字段交换改变有序投影；普通注释空白无 diff，raw string/宏换行有 diff。 | 未执行。 |
| 重载/身份 | 同文件两个重载、重复 prototype、同名 static 函数分属两个 `.c`、匿名 namespace、局部 class；插空行、更换根、增减选集 | 不冲突，未变对象身份稳定；参数类型变更的增删符合约定；USR 不能导致位置漂移。 | 仅 LSP 重载已跑。 |
| 声明定义 | `api.h` prototype＋`a.c` definition；C++ 类内声明＋类外定义；仅选 header/仅选 source/同时选 | 保留各自文件内容，目标选择不随选集猜测；跨文件源片段不重复。 | 仅 LSP prototype 已跑。 |
| 调用与指针 | 直接调用/取地址/重载/虚方法、函数指针参数、成员指针、可变 alias、`extern "C"`、`sizeof/decltype/noexcept` 未求值调用、隐式析构 | Node/External/Unresolved 正确；definition 到参数不能成为 Callable；未求值/隐式调用按固定政策处理；不声称完整运行时分派。 | LSP 三个查询已跑，其余未执行。 |
| 模板与生成代码 | primary/partial/full specialization、显式实例化、依赖调用、token paste 宏、宏生成多个函数、`#line` | 不重复实例化节点；主模板不继承某一实例的唯一边；宏原文和来源可信、无虚拟路径归属。 | 仅简单宏位置已跑。 |
| 配置/头文件 | 同 header 被两个不同 `-D`/语言模式 TU 使用；调换 CDB entry 顺序；非 self-contained header；新文件不在 CDB | 不按第一条/相似名字确定配置；互斥分支不混图；输入命令来源可追溯。 | 条件分支 AST 已跑，其余未执行。 |
| 构建数据库 | CMake/Ninja、Meson/Ninja、MSVC/Ninja、Bazel 提取器各一小项目，消费已有输出；空格/Unicode路径、`@rsp`、绝对 `-I`、forced include | 只读导入；有效 mode/target/include 一致；不能执行 command；源根只指本侧。 | 未执行，未运行任何项目 configure/build。 |
| 缺上下文 | 分别移除 resource headers、SDK、第三方 header、第三方 implementation、PCH/BMI | 包损坏、上下文缺失、已知声明无实现分开；错误有 AST也不能成功空图；可用对象保持的条件有明确断言。 | 仅 missing-header 反例已跑。 |
| 路径与环境 | CDB指向主工作树/head；父 `.clangd`、用户config、symlink/junction、环境 include、wrapper/query-driver；故意毒化 | 不能跨侧读取；禁止的driver不启动；映射/依赖错误明确；测试不更改真实业务数据。 | 未执行。 |
| 支持包 | macOS ARM64/Windows x64 安装固定包，缺文件/错误hash/迁移位置/正在使用时删除 | 实际 C/C++ AST和语义查询通过；resource-dir可定位；二进制无开发机隐式库依赖；租约有效。 | 未执行固定包。 |
| 进程与错误 | AST大TU期间取消；初始化/请求超时；helper崩溃；spawn子进程后故意挂起；refresh/close | 原错误优先，所有后代退出才释放工作区；无成功部分图；Windows Job原生验证。 | 仅正常 clangd shutdown已跑。 |
| 索引与性能 | 选1个文件/100个文件；background-index关/开；冷/热；改一个被多TU使用的header | 记录耗时/RSS/缓存和读取范围；失效正确，索引不越侧；根据实测决定预算。 | 未执行，不给虚构性能数字。 |
| modules | `.cppm`、partition、header unit、缺BMI、错版本PCM、MSVC IFC | 已宣布支持的profile才可成功；缺上下文明确报错；不从另一侧或旧构建复制BMI。 | 未执行。 |
| 产品集成 | Rust/TS/Python/C/C++混合、仅删C文件、header模式、测试开关、缺包重试、宽/窄窗口长名称 | coverage真实；语言视图/边/源码展示正确；工具失败保留当前Review Snapshot。 | 未执行。 |

LSP 可复现输入为：`api.h` 一行 `extern "C" int c_api(int);`，`main.cpp` 四行分别为 include、`int pick(int x){return x;}`、`double pick(double x){return x;}`、`int use(int (*fp)(int)){return pick(1)+c_api(2)+fp(3);}`。CDB `arguments` 指定 `clang++ -std=c++20 -c main.cpp`，`directory/file` 使用临时目录真实绝对路径。按 LSP `Content-Length` 分帧依次 initialize（UTF-16）、initialized、didOpen，然后定位第 4 行三个 token 的 character 发 `textDocument/definition`；期待 int overload、header prototype、参数三个范围，最后 shutdown/exit 并 wait。下一轮应把同夹具变成仓库真实服务测试；本轮没有额外提交探针文件。

## 12. 具体接入文件与交付阶段

以下路径为基线真实入口；新增路径为建议。结构变化应位于自己的深模块，不把 Clang 的 AST 类型扩散进公共 Code Graph。[ADR-0019](../adr/0019-organize-rust-backend-by-capability.md)

| 接入点 | 所需工作 |
| --- | --- |
| 新 `src-tauri/src/structure_review/c_cpp_analyzer/` | 实现 `LanguageAnalyzer`，负责输入来源边界、helper 协议与进程、通用 DTO 校验、最终图构造和生命周期；不重新实现 C/C++ 身份、内容投影或关系分类。测试在模块 `tests/`，中文类型/方法注释。不要先引入通用插件框架。 |
| 新的原生 helper 构建目录（具体布局待实施决定） | 小型 C++ LibTooling 程序，固定构建并分发；同进程取得语法语义并完成类别、内容、身份、图成员与关系目标分类，返回统一接口的规范化构图 DTO。字段 schema、诊断和完成标志仍待设计，输出有界，Rust 校验避免不完整结果。源码文件只能是 Narrant 工具本身。 |
| [structure_review/mod.rs](../../src-tauri/src/structure_review/mod.rs) | 导出新适配器；公共六类/图比较通常不必改。若增加配置/降级能力报告，另审公共契约。 |
| [application/mod.rs](../../src-tauri/src/structure_review/application/mod.rs) | 扩展 `LanguageAnalyzers`、`SideFiles`、`LanguageGroup`、`language_group`、包需求、两侧worker合并；`.c/.cc/.cpp/.cxx/.h/.hpp/.hh/.hxx`候选路由到共享组，实际模式由编译配置判断。扩展名 `.C` 大小写和 modules 另明确。 |
| [tauri_app/bootstrap.rs](../../src-tauri/src/tauri_app/bootstrap.rs) | 装配开发/生产入口和取消信号；依赖范围用新 C/C++ policy，勿复用 Rust枚举。生产可执行路径由 PackLease 固定。 |
| [lsp/mod.rs](../../src-tauri/src/lsp/mod.rs)、[transport.rs](../../src-tauri/src/lsp/transport.rs)、[source_text.rs](../../src-tauri/src/structure_review/source_text.rs) | clangd直接复用协议、坐标、进程回收；LibTooling helper需自有协议或提取共享进程守卫，不把非LSP JSON硬接现有客户端。 |
| [language_support/model.rs](../../src-tauri/src/language_support/model.rs)、[storage.rs](../../src-tauri/src/language_support/storage.rs) | 增加 `PackId::CAndCpp`/slug/required入口/历史版本扫描；校验resource headers和动态库；确认包大小上限，保持安装原子性与使用中删除保护。 |
| [resources/language-support-packs.json](../../resources/language-support-packs.json)、[build脚本](../../scripts/build-language-support-packs.mjs)、[publish脚本](../../scripts/publish-language-support-packs.mjs) | 固定LLVM/helper版本、上游源码/产物/许可证摘要；扩展包ID白名单/默认构建列表、两平台构建与原生smoke；发布脚本和manifest tests一起核对。 |
| [src/language-support/types.ts](../../src/language-support/types.ts)、[包卡片](../../src/components/language-support/language-support-packs.tsx)、[错误白名单](../../src/structure-review/use-structure-review.ts) | 增加C/C++包名和稳定错误处理；若UI需要配置选择/降级说明，单独设计，不能将内部Clang参数塞成普通用户流程。 |
| [language-view.ts](../../src/structure-review/language-view.ts)、源码编辑器映射、[Preview Harness](../../src/dev/preview.tsx) | 语言视图已有数据驱动过滤；验证 C/C++ 跨语言边、共享header模式、文件/对象归属和长模板名；新增通用场景，真实浏览器验证布局。 |

特别注意：当前 [Babysit 扩展名检测](../../src/babysit/detected-language.ts) 已把 `.h` 简单映为 `cpp`，那是另一功能的语言提示，不是 Structure Changes 的语义资格。不能直接复用这个规则证明 C header 应按 C++ 解析。

建议分阶段交付：

1. **范围与原型（先决）**：在已决架构、禁止执行与默认依赖边界内，确认第 13 节的配置、目标与声明映射细节。锁定候选 LLVM，完成相同 TU 上 C/C++ modes、宏归属、重载/声明片段与路径重映射原型；拿实际失败案例决定 active-only 或 helper 内原始语法辅助路线。没有通过则不启动包发布。
2. **源对象与差异**：在限定 profile 下完成六类、稳定身份、自身内容和公共图校验；仅 fixture 运行也必须真实Clang，不靠mock模拟宏/模板行为。选集、空文件、测试规则与硬失败有单元/集成断言。
3. **项目与语义**：内存编译数据库、多项目/头文件、明确目标 SDK、helper 内关系证据/外部身份和 Rust 生命周期回收。先有界 AST，量化跨 TU 缺口后再决定 helper 内索引；独立 clangd 保留为研究比较，不在主链路跨进程拼接语法语义。
4. **固定支持包与应用**：原生两平台构建、安装/删除租约、混合语言比较、配置/能力提示、Preview Harness真实渲染。类型检查、Lint、生产构建和适当Rust测试仍必须完成。
5. **扩展能力**：多配置视图、更多标准/平台、modules 或大型缓存，在已决来源与禁止执行边界内逐项独立验收；不通过执行项目生成器补齐。不能将阶段5缺口隐藏为阶段4的“全量支持”。

实施时所有 Cargo 走 `scripts/cargo-agent.sh`，优先 `check --lib` 与聚焦测试，按仓库节奏控制全量测试；不手动触发 quality-gate。本文仅调研，无 Issue/PR/提交/切换分支或业务代码变更。

## 13. 待用户决定的产品问题与 ADR 冲突清单

架构、禁止执行与默认依赖范围已经由 ADR-0034 确定；下表保留这些约束并列出仍需写入 C/C++ 规格的配置、目标矩阵和映射细节，不能把其余建议说成用户已经选择。优先处理 Q1–Q4。

| 问题 | 推荐起点 | 取舍/原因 |
| --- | --- | --- |
| Q1：承诺所有显式源声明，还是一个明确配置下的活动声明？ | 首个可交付profile先选明确配置，并展示其边界；若需求必须覆盖所有分支，则先验证原始语法辅助层再承诺 | 纯Clang AST会删除未活动分支；现有0018完整性期望不能无提示收窄。 |
| Q2：明确目标的标准库/平台 profile 如何提供？ | 默认范围已决：仓库源码＋明确目标标准库/平台声明；具体 SDK/标准库版本、来源与支持矩阵仍待定 | 不下载/restore 第三方，不自动纳入本机已有业务依赖；已提交源码按项目/选集规则。缺必要头文件可能使 TU 无法完整提取，不能据此放宽范围。 |
| Q3：无CDB/多条命令/孤立header如何处理？ | 简单文件可显式独立profile；复杂/歧义项目要求配置/TU选择 | 不能仅按后缀、第一条CDB、clangd相似文件名声称正确；是否允许结构降级需要新的能力/完整性呈现。 |
| Q4：声明与定义怎样显示？ | 各源片段保留对象与文件归属，内部关联同一实体；首版不新建公开对应关系 | 合成一个跨文件节点会影响选集与自身内容；当前单父/单source不自然支持多文件实体。 |
| Q5：宏生成对象、模板实例化和局部声明粒度？ | 显式源码优先；不生成隐式实例；lambda留owner；具名局部类型是否独立按夹具确认 | generated/prototype与真实用户声明不能混淆；和Python/Rust不同必须明说。 |
| Q6：C/C++标准与方言、测试识别范围？ | 先C11/C17、C++17/C++20；C99/C23/C++23、GNU/MSVC扩展列追加矩阵；默认只排除可证明的专用测试路径/目标 | 不自动支持Clang所有语言；不因名字叫test就排除生产代码；GoogleTest宏/生成测试需独立规则。 |
| Q7：modules和后台索引是否首版必需？ | modules延后专项；后台索引默认关，有界TU优先 | 版本化BMI/生成上下文和大仓库资源成本明显；若用户主要代码依赖modules，阶段顺序必须调整。 |
| Q8：缺SDK时是否允许独立结构模式？ | 提供明确错误与修复入口；只有充分验证和UI可见标识后才开放降级 | 现行成功图没有“半完整”含义，不能自动回退后吞掉声明。 |

明确冲突：复制源码/头文件/清洗后配置、另开语言checkout、借读用户主工作树、跨侧复用生成物或租约外索引违反 ADR-0031；自动替换用户编译器/安装SDK违反 ADR-0032 的工具链边界；引入第七种公共对象类别违反 ADR-0030；用SDK缺失/语义失败制造成功空图违反共享 AnalysisError 约定。当前 graph没有配置维度、`.h` 双模式、跨文件多声明和继承关系词汇是需要明确的新设计，不是本调研已修改的决定。

若语言服务不可用，当前 Review Snapshot 和 File Changes 仍应可用，不能回滚已激活Review；这遵守 [ADR-0012](../adr/0012-separate-review-refresh-from-analysis-readiness.md)。完成C++能力后，只有同样通过C专属对象、关系、语言模式及失败路径验收，才可以对外称为“C/C++支持”。

## 14. 后续配置并集探针（2026-10-02）

本节是在 ADR-0034 已接受“一个配置并集图”之后追加的实验，不采用前文保留的单配置结果封套、多配置界面或强制用户选一个配置建议。已执行 [可复跑探针](probes/c-cpp-native-helper/README.md)，结果保存在 [observations-2026-10-02.json](probes/c-cpp-native-helper/observations-2026-10-02.json)。工具为本机 Apple Clang 21.0.0；Python 驱动在 `/tmp` 生成独立夹具，固定 `FAST=0/1`，分别以严格 C17 / C++20 执行 JSON AST dump。没有构建 Narrant、LibTooling 或产品 helper，没有运行夹具程序、仓库构建、依赖下载或 SDK 安装。

**实测支持普通调用关系的单图并集，同时说明不能把所有宏变化都当成对象冲突：**

- 同一 `int run(void) { return PICK(); }` 在两配置下保持同一原始源码范围和文本；`PICK` 分别绑定 `fast`、`safe`。每条调用通过 `CallExpr → DeclRefExpr.referencedDecl.id → FunctionDecl` 的后端证据确认，因而一个源码对象可以保留两条关系，不能把关系并集误解为同时执行两次调用。
- `#if` / `#else` 各有一个同名 `mode` 定义时，它们来自原文的两个不同声明位置。两次解析分别给调用绑定正确的声明。它们是可分别保留的源片段，不能按同名、链接名或全局实体键直接覆盖；生产稳定 NodeId 尚未验证，探针偏移仅是当前不可变源码内的证据。
- 同一 `RESULT value(void)` 的推导返回类型分别为 `int`、`double`，但其原始源码签名都包含 `RESULT`。按源码投影可避免展开类型造成的假内容冲突；这不意味着忽略推导类型的语义作用，也不允许任选一个类型写入必须单值的图字段。正式映射若把解析类型纳入身份、签名或扩展值，仍须独立处理冲突。

**仍须按 Q5 失败的真实对象冲突也已复现：** 同一 `DECL item;` 因宏成为 TypedefDecl / VarDecl，对应 Type / Value，原文相同不能消除类别冲突；同一 `int slot;` 位于条件编译选择的 Left / Right 两个 struct 内，如果字段独立建对象，就产生两个父对象，不能满足单父包含约束。对这些承诺支持的源对象，不能取首份、复制成配置专属对象或降为不完整成功图。

所有 AST 夹具都在另一个临时根目录颠倒配置顺序重跑，归一化证据一致；调用前后夹具文件与内容摘要不变。另一个 C/C++ 模式反例仍为严格 C17 成功、严格 C++20 失败，说明共用后端不等于取消语言模式区分。这里验证的范围非常有限：不证明 Clang 所有功能全局只读，不证明跨编辑稳定身份，不证明自动配置发现或图级完整性；JSON AST 只是测试观察入口，**尚未验证 LibTooling API、完整 own-content 投影、协议或产品实现**。上述证据足以继续原型，不支持声称整个 C/C++ 方案已经验收。

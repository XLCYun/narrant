---
status: proposed
---

# 原生语言分析配置与验收草案

本文细化 [0024 通用辅助进程协议](0024-native-analyzer-helper-protocol.md) 的语言配置、
源码映射和验收输入。共同架构和单图并集语义已经由
[ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 确认；
以下 profile 是实施候选，**没有完成实验的版本、工程形态和平台不构成发布承诺**。
Rust、TypeScript/JavaScript、Python 的既有实现不变。

## 共同输入与发布门槛

一个分析 profile 固定后端与投影版本、源码归属规则、语言规则、目标平台声明、
静态配置解释规则及测试识别规则。profile 标识描述能力，不直接编码支持包补丁版本；
具体运行构件和目标声明均另记精确版本、摘要及允许来源。

| 必须分开的输入 | Java | C# | Go | C/C++ |
| --- | --- | --- | --- | --- |
| 执行 helper 的工具 | 私有 Java runtime、JDT 依赖闭包 | 私有 .NET runtime、Roslyn | 已编译 helper 的 Go runtime | 已编译 helper、Clang 库及 resource headers |
| 解释源程序的规则 | source/release、preview、模块设置 | LangVersion、常量、Nullable、unsafe | 语言版本、GOOS/GOARCH、tags、cgo | C 或 C++、标准/方言、target、宏 |
| 目标平台上下文 | 对应目标的 Java SE 或明确平台声明 | 对应 TFM 的 reference assemblies | 对应目标 SDK 的标准库 | 对应 target 的系统/标准库头文件与 ABI |

Host 传入经校验的目标平台目录 catalog；helper 只能从中匹配 profile 需要的声明。
缺少匹配目标时失败，不改用当前机器或 helper runtime 的默认标准库。
支持包安装与项目 SDK 提供遵循 [0022](0022-language-support-packs.md)：
本草案不授权自动安装项目 SDK、workload、业务依赖或执行构建。
目标声明是否随包分发，要以许可、体积和目标隔离实验决定，不能由“工具自带 runtime”推导。

配置发现只解释有完整定义的有限静态子集。每个发现结果保留内部来源和有效输入摘要，
两侧使用相同解释规则、分别读取本侧文件。未知语义可能影响源码成员、语言规则或目标时，
拒绝该 profile；不可取第一个 XML 值、忽略条件、拼接整个 monorepo 或默默降级。
静态可读不等于语义已知，例如 Gradle 脚本里的字面量仍可能被后续代码改写。

多配置在 helper 内分别取得事实，再按 0024 归并为一张图。没有配置切换界面，
不穷举任意宏组合。只报告实际分析过的有限配置范围，不能将其称为“所有配置”。
同一源声明跨配置使用同一 ID；两个互斥分支中的不同源声明可以有不同源码身份，
但不得仅为规避冲突将同一声明复制成带配置 ID 的节点。

项目及依赖提供的构建逻辑、插件、处理器、生成器一律不执行。
业务 JAR/NuGet/模块缓存/安装目录不自动成为语义上下文；仓库已提交源码按语言归属规则处理。
生成器没有运行产生的缺失关系可为 Unresolved；缺少源码清单或必要头文件导致对象不完整则失败。
只有两侧固定源码、允许的平台声明和受控工具可参与分析，缓存不得跨侧补齐缺失生成物。
`include_tests=false` 只排除可信静态 source-set/项目元数据或符号证据确认的测试专用对象；
多配置共享生产/测试的对象保留。缺框架引用时不按 `Test` 拼写删方法，未知和共享辅助代码保留。
选项为 true 仅恢复原选集中的测试对象，不扩大用户选集，也不运行测试发现。

## 首版 Platform catalog 字段

以下是 `*-static-v1` 实现共同使用的工程接口。字段冻结不表示相应系统已通过验收；
实际验证范围以语言实现记录和跨平台执行结果为准。所有 root 均为明确目标声明目录，
不得通过扩大到磁盘、用户目录或 Program Files 根目录来包含多个不相干的来源。

| kind | root 与版本 | metadata |
| --- | --- | --- |
| `java-jdk` | 精确目标 JDK 根，包含 `release` 与 `lib/jrt-fs.jar`；version 为实际完整版本 | `{release: string}`；release 必须与目标 JDK feature version 相符 |
| `go-sdk` | 固定 Go 标准库源码根，包含 `src` 与 `VERSION`；首轮 1.23.8 | `{goos: string, goarch: string, goamd64?: string}` |
| `dotnet-reference-assemblies` | 精确 `ref/<tfm>` 目录；首轮 10.0.0 | `{tfm: string, reference_pack: string, reference_pack_version: string}` |
| `clang-target` | 精确系统 SDK 根；version 为该 SDK 实际版本 | `{target_triple: string, c_standard: string, cpp_standard: string, system_include_directories: string[], resource_platform_id: string, additional_system_platform_ids?: string[]}` |
| `clang-resource` | 固定 LLVM 的 `lib/clang/<major>` 目录；首轮 21.1.8 | `{}` |
| `clang-system-headers` | 与系统 SDK 分开放置的精确标准库头文件根，例如 Windows MSVC 的 `include`；version 为其实际工具版本 | `{}` |

`system_include_directories` 只允许相对于本条 `clang-target.root` 的路径。
`resource_platform_id` 必须引用 catalog 中的 `clang-resource`；可选
`additional_system_platform_ids` 只能引用 catalog 中的 `clang-system-headers`，
不接受重复、空 ID、自引用或未知 ID。缺省表示无额外系统头文件集合。
这些条目只补充一个目标的标准声明，不单独创建分析配置，也不授权第三方 include、
编译器命令、wrapper 或环境搜索路径。Host 与 helper 均校验关联。

Windows 的系统 SDK 与 MSVC 标准头通常位于两个目录，因此分别建 catalog 条目；
macOS 目标按支持包控制文件指定的精确 SDK 版本，从约定的 Xcode/CLT 安装位置匹配。
CI 与本机可验证不同的明确 SDK 版本，但不能把单个版本通过写成所有版本通过。

## Java：JDT Core 候选

JDT Core 是首个实验后端，javac 是研发对照及可能的替代选型；生产只选一个，
不要求每次双引擎运行，也不自动回退。使用独立 `ASTParser` 环境取得语法与 bindings，
显式关闭运行 VM bootclasspath 的隐式纳入；配置 sourcepath、目标平台和语言选项。
原始字符输入还需要匹配的 unit name。API 能表达这些输入，但目标 API/JPMS 的正确性仍须实验。
[JDT ASTParser](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/ASTParser.html)

| 候选 profile | 静态输入与范围 | 拒绝或暂缓的输入 |
| --- | --- | --- |
| `java-source-root` | 一组可证明属于同一项目的明确源码根、编码、目标 release；校验 package 与根路径；先验证普通非模块项目 | 不知道目标 release、互相冲突的重复类、未知源码根；不从当前 JDK 推断目标 |
| `java-maven-static` | 仓库内 POM 的字面值、有限属性替换、仓库内 parent/modules、明确 main/test roots；解释规则与默认值固定并有夹具 | 外部 parent/BOM、未知 profile 激活、影响源码/编译规则的插件或未知属性；不运行 Maven effective-model |
| `java-gradle-metadata` | 仅消费已在本侧固定、来源与 schema 已验证的声明式分析元数据 | 不执行 Gradle/Tooling API；首版不靠匹配 build.gradle 文本复现工程模型 |

先用一个非 preview 的明确 Java SE 目标验证基础闭环；Java 17 可作首个实验目标，
再用 Java 8 的 `List.of` 反例验证工具 runtime 与目标 API 隔离。这是实验选择，
不是承诺完整 Java 8/17 支持。JPMS、Android/Jakarta、preview、compact compilation units、
module import 均独立验收；相关源文件被选中时不能跳过后返回普通成功。
不导入 JDT LS/m2e/Buildship，不运行 APT、Lombok agent 或 delombok。

| 类别 | 建议源码对象 | 内容归属 |
| --- | --- | --- |
| Container | file、package fragment；通过专项验收后再纳入 module | 文件只组织本文件；package 保留显式声明与注解，不合成全仓包节点 |
| Type | class/interface/enum/annotation/record，显式局部与匿名类 | 类型头部与初始化次序属于 Type；独立子对象内容剥离 |
| Callable | method、constructor、compact constructor、annotation member | 每个重载独立；签名和实现分开，无体与空体不同 |
| Value | field、enum constant、record component | 多变量逐声明；各自拥有 initializer；组件不重复制造隐式访问器 |
| ExecutionRegion | static/instance initializer | 每块独立；与字段的执行次序由 Type 的有序锚点保留 |
| Extension | import；模块专项中的 module directive | 保留有意义的源码声明，不把 import 拼写直接当已解析关系 |

普通局部变量、参数与 lambda 留在最近独立对象的 code 中，内部引用归该 owner；
显式局部/匿名类独立，父表达式保留构造实参和子对象锚点。
record component 的内容归组件，父 record 保留有序组件锚点，避免重复比较。
默认构造器、bridge、record/enum 隐式成员不补造源码 Node。

Java ID 使用语言、相对文件、词法 owner、kind、名字及源码参数类型重载槽。
重载槽不依赖外部类型解析；泛型参数按位置规范化。参数名、返回类型、throws 和注解变化
通常只改变内容；参数类型拼写变化可以保守表现为删除/新增。JDT binding key 仅作会话内关联。
匿名类、重复初始化块采用固定源码槽位规则，插入无法区分的兄弟可能造成保守替换。

`code.calls` 指向绑定确认的静态 method/constructor 声明；方法引用用 `code.references`。
显式父类/父接口用 `code.extends`，显式接口实现用 `code.implements`；
`code.member_of` 表达确认的成员归属。`code.type_use` 先限字段/record component 类型，
`code.imports` 只输出已确认目标；未知、歧义或 recovered binding 不冒充 External。
不推断反射、框架注入、动态 override 集合或字段读写图。

## C#：Roslyn compiler API 候选

使用语法树、显式 reference metadata 和 options 创建 `CSharpCompilation`；不经
MSBuildWorkspace。运行 helper 的 runtime 与目标 reference assemblies 分开，
不能引用 helper 的 `System.Private.CoreLib` 来代替项目 TFM。平台程序集按 metadata 读取，
不作为插件加载执行。[Compilation API](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.csharpcompilation.create?view=roslyn-dotnet-4.13.0)、
[Reference assemblies](https://learn.microsoft.com/en-us/dotnet/standard/assembly/reference-assemblies)

| 候选 profile | 静态输入与范围 | 拒绝或暂缓的输入 |
| --- | --- | --- |
| `csharp-source-set` | 明确同一编译单元的源文件集合、TFM、语言选项、预处理常量、reference profile | 不把全部 `.cs` 当一个项目；未知编译成员或未提供目标不得静默补默认 |
| `csharp-sdk-static` | 普通 Microsoft.NET.Sdk 项目的已验证有限子集；字面 TFM/TFMs、LangVersion、Compile Include/Remove、ProjectReference 及有限条件 | 自定义 SDK/import/tasks、属性函数、未知条件或影响 Compile 的 targets；不运行 MSBuild 求值 |
| `csharp-static-metadata` | 本侧已有且 schema/来源验证通过的声明式源码/选项元数据 | 不借用另一个 checkout 的 obj/assets，不从 NuGet cache 扩大允许来源 |

`csharp-sdk-static` 的默认 glob、排除项、隐式常量和 implicit usings 必须由固定且已验证的
SDK profile 定义；缺这些规则时该 profile 尚不可用。内存生成的标准 implicit usings
只作有来源的分析上下文，不作为仓库 Node，也不写回工作区。
有限条件解释从已列举的 Configuration/Platform/TFM 等值匹配开始；不能执行属性函数，
也不盲目对全部变量做笛卡尔积。所有明确且受支持的分支分别分析，再合并。

沿用研究中的 C# 14 / net10.0 作为一个原型候选，Roslyn 与 runtime 的精确版本需锁定后实测。
这不承诺 C# 7.3–14、.NET Framework、Unity、Razor、脚本或所有 TFM 同时可用。
已提交 `.g.cs`/`.Designer.cs` 按普通源码规则处理；不开 generator/analyzer、restore 或 test discovery。

| 类别 | 建议源码对象 | 内容归属 |
| --- | --- | --- |
| Container | file、namespace declaration part | using/alias/global attributes/指令进入规范化文件内容；跨文件 namespace 不合成多父节点 |
| Type | class/struct/interface/enum/record/delegate，显式嵌套类型 | 每个 partial 片段独立；primary constructor/record 参数保留在类型头部 |
| Callable | method、constructor、destructor、operator、conversion、显式 accessor | 属性 expression body 投影为 getter；无源码隐式实现不制造 code |
| Value | field、constant、enum member、property、indexer、event | initializer 属于 Value，accessor 为子对象；不生成 backing field |
| ExecutionRegion | top-level statements | 不制造隐式 Program/Main；保留原始顶层语句顺序 |
| Extension | 已验收语言版本的 extension block | 保留 receiver/约束与显式成员，不把成员迁到 receiver 类型下面 |

local function、lambda、anonymous method 和普通局部变量首版内联于最近 owner；
其调用仍被遍历。async/iterator 保留源 Callable，不生成 MoveNext/闭包类。
字段、auto-property、field-like event 初始化次序保存在各类型片段的有序投影中，
不制造跨 partial 文件的全序；enum 隐式成员顺序必须影响相应内容。

C# ID 使用相对文件、词法 owner、kind、名字、泛型 arity、源码参数类型/passing mode、
显式接口限定与 partial role；转换运算符另区分目标类型。参数名不参与重载槽，
泛型参数按位置规范化，accessor 按 get/set/init/add/remove 角色区分。
不使用 Roslyn ProjectId、symbol hash、行号或绝对路径。普通 overload 重排不改变 ID。
partial type 保留全部源片段；同一已确认符号可映射多个声明片段，不能挑第一个充当唯一源码。
partial member 的声明/实现配对和调用映射必须有单独夹具；缺生成实现不能假装合法擦除。

`code.calls` 先覆盖显式 invocation、object creation、构造初始化器及已验证的源码运算符调用；
method group 用 `code.references`。virtual/interface 只指静态目标，dynamic/未知 delegate
调用保留 Unresolved。LINQ、collection initializer 和新版语言隐式调用逐项验收后加入范围。
继承语义统一采用现有实现的 `code.extends`，不引入研究旧稿的 `code.inherits` 别名；
另用 `code.implements`、`code.member_of`，字段/property/indexer/event 的窄范围 `code.type_use`。
using 参与内容与绑定，首版不发 imports；不做一般数据读写边。

## 条件源码、自身内容与目标归并

源码内容投影须尽量由同一原始声明确定，不因不同配置的预处理 AST 改写同一对象的 code。
例如方法体含 `#if A ... #else ...`，候选方案保留条件指令与两支的原始规范化 token，
各配置仅贡献其证实的关系。该投影算法仍需证明不吞字面量、注释内指令或独立子对象。
非活动源码不自动变成具有语义绑定的节点，也不为展示非活动文本新增 variant 节点。

两支分别声明同名方法时，它们是两个实际源声明；通过可复验的词法分支/声明槽位区分。
同一声明在不同配置下出现无法兼容的类别、签名或包含位置时，按 0024 返回归并失败。
同一个源码引用在两种有效配置下分别绑定 A/B，可以保留两条已解析关系；
某配置仍未解析的观察不能因为另一配置成功而被删除。
Node/External 最后依据并集图成员重新分类，不能沿用中间配置的选集结论。

所有语言使用词法/AST 感知的规范化，纯排版与普通注释不产生自身内容 diff；
保留字符串、数值、指令、构造顺序和语义相关 token。独立子声明替换成稳定锚点，
子方法体修改不能污染父 Type。Java Unicode escape/text block、C# raw string/NEL、
C/C++ 预处理续行均有独立夹具。范围转换绑定同一 source 摘要，不用 `#line` 虚拟路径作身份。

## Go 与 Clang 的首轮配置概览

| 后端 | 候选静态 profile | 源对象与关系约束 | 必须关闭的实验项 |
| --- | --- | --- | --- |
| Go parser/ast/types，受限源码 importer | 优先验证静态项目模型：仓库内 go.mod/go.work 的受支持子集、固定 SDK、明确平台/tags；先无 cgo | 按文件/包上下文提取显式 type/function/method/value；init 保持 Callable；共享初始化只有一个 owner；接口实现须有类型证据，函数变量不冒充直接调用 | 手工包映射扩展到真实项目、标准库源码闭包、离线缺依赖、workspace/module 边界、平台/tags 的有限发现；默认 packages driver 仍有只读阻断 |
| Clang LibTooling | 本侧已有且可验证的 compilation database；按 TU 的受控参数解释；简单无 CDB 模式单独验证 | C/C++ 共用 helper、分别保留语言模式；prototype/definition 源片段、宏来源、模板声明与实例区分；函数指针目标不由同名猜测 | 不执行 command/wrapper/query-driver；参数与 response file 白名单、缺头完整性、共享 header 多上下文归并、target SDK |

Go 的 package ID/位置、Clang 的原始 USR 均不是未经验证的持久 NodeId。
Go 版本先锁定单一实验 SDK；Clang 可从 C17/C++20 两个独立模式实验，不能因此声称所有 C/C++ 方言。
C 不由“C++ parser 接受了代码”自动覆盖；两者的类型规则、预处理与标准库必须分别验收。
详细背景见 [Go 调研](../research/go-language-support.md)、[C/C++ 调研](../research/c-cpp-language-support.md)。

2026-10-02 的 [Go 原生探针](../research/probes/go-native-helper/README.md) 已在手工指定包映射和
两个平台的 fixture 中验证源码 importer 能取得仓库包、部分标准库绑定，并拒绝磁盘上已有的
第三方源码；分析阶段未调用 go list，fixture 前后路径和字节不变。这支持优先验证受限静态
loader，尚未解决真实 go.mod/go.work 解释、完整标准库、跨版本或操作系统级只读保证。
[C/C++ 原生探针](../research/probes/c-cpp-native-helper/README.md) 在 C17/C++20 两模式验证了
同源调用边并集、原始签名避免宏展开假冲突，以及必须失败的类别/单父冲突。
它使用系统 Clang JSON AST，尚未验证 LibTooling、生产身份算法或自动配置发现。

## 验收矩阵与证据记录

每个用例保存合成源码、静态配置、工具/target 摘要、精确 expected 图或错误、原始输出。
验收在临时夹具执行，禁止为了通过而下载业务依赖、运行被分析代码或改写真实仓库。

| 验收 | Java 必测 | C# 必测 | 共同通过标准 |
| --- | --- | --- | --- |
| 目标隔离 | 新 runtime 分析旧目标，List.of 不可误绑定 | helper runtime 与不同 TFM refs，目标没有的 API 不可误绑定 | 目标缺失明确失败；无宿主默认补齐 |
| 基础语义 | 跨文件、重载、泛型、匿名类、method reference | 跨文件、overload、partial、dynamic、accessor | 只依据证据连边，无同名/候选猜测 |
| 完整性 | 缺业务类型与 malformed AST 分开 | 缺 NuGet 类型与语法恢复树分开 | 声明完整可携带 Unresolved；对象缺失必须失败 |
| 身份/内容 | record、重载重排、字段/块初始化次序 | partial 片段、显式接口、字段/属性次序 | 两 checkout 同 ID；body-only、child-only 与顺序变化归属正确 |
| 并集 | 同源文件多个 source sets、目标 API 不同 | 两常量/两 TFM、partial 条件实现 | A/B 边可并集；冲突失败；顺序不改变结果；保留未知观察 |
| 静态配置 | parent/property/profile 不明、Gradle 脚本 | 未知 import/Condition/SDK、linked source | 不静默忽略影响范围的配置，不执行求值器 |
| 禁止执行 | 构建插件、APT、Lombok 的哨兵夹具 | targets、analyzer、generator 的哨兵夹具 | 从加载到退出均未执行哨兵，无依赖安装 |
| 只读/来源 | 源码根、classpath、外向链接、环境污染 | source/ref roots、父目录配置、环境污染 | 无工作区写入或瞬时写后恢复；只读许可来源 |
| 生命周期/包 | JDT 取消与强制终止、私有 runtime | Roslyn 取消与强制终止、私有 runtime | 后代回收才释放租约；无宿主工具仍能启动已承诺模式 |

首批原生平台沿用支持包基线 macOS Apple Silicon、Windows x64，分别运行真实 helper 验收；
“文档有 API”或单机成功不替代另一平台的启动、路径、只读与取消测试。
Java JDT/javac 目标对照、C# compiler API 最小探针仍未在本轮完成；
Go/Clang 的已有局部实验也不是完整发布验收。上述空项是工程实验任务，不重复要求用户确认。

参考背景：[Java 调研](../research/java-language-support.md)、[C# 调研](../research/csharp-language-support.md)、
[Code Object](0018a-code-object-kinds.md)、[Code Relationships](0018b-code-relationships.md)、
[Rust Interface](0018c-rust-interface.md)。本文明确的单图并集规则覆盖研究旧稿中的单配置结果、
冲突一律降为未知和配置变体界面候选；其他研究 API 事实仍需按固定构件复验。

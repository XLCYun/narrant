# 原生语言分析器实施计划

状态：新增四语言和共享Windows回收修复均完成，`83d5642`的完整双平台矩阵四组全部通过；PR为草稿，未合并或发布。
交付包括 Java、Go、C#、C/C++ 原生 helper 及应用接入。依据 [0024](0024-native-analyzer-helper-protocol.md)、
[0024a](0024a-native-language-analysis-profiles.md) 与 ADR-0034。

实施票据：[公共适配器 #193](https://github.com/XLCYun/narrant/issues/193)、
[Go #194](https://github.com/XLCYun/narrant/issues/194)、
[Java #195](https://github.com/XLCYun/narrant/issues/195)、
[C# #196](https://github.com/XLCYun/narrant/issues/196)、
[C/C++ #197](https://github.com/XLCYun/narrant/issues/197)、
[应用与支持包 #198](https://github.com/XLCYun/narrant/issues/198)、
[贯通验收 #199](https://github.com/XLCYun/narrant/issues/199)。

## 当前实施状态（2026-10-03）

共享回收后续工作已接入公共owner并通过完整生产路径验收：
[运行37128777728](https://github.com/XLCYun/narrant/actions/runs/37128777728)四组全部成功。
Windows共享owner6项、Transport2项、LSP13项、原生Host17项、安装及8图均通过，
Python23/3/4也通过（含3项真实固定ty会话）。macOS两组通过；本地Rust753项与20项脚本测试通过。
具体证据及边界见[跨平台记录](../research/native-crossplatform-validation.md)。
分项与全分支审查的范围、修复及关闭情况见[审查记录](../research/native-analyzer-review.md)。

### 新语言交付基线（4c65bb8，历史记录）

`4c65bb8` 的[完整运行 37111018854](https://github.com/XLCYun/narrant/actions/runs/37111018854)
已取得新增原生链路的双平台完整证据：四包构建/ready、Go、Java25、C#29、Clang25项均通过，
Clang每个平台各有1项仅适用于另一平台的SDK测试跳过。
Windows Host19项通过（另1个ignored函数仅为子进程夹具）、factory2、生产安装1/207.19秒、
公共图8/22.48秒通过；macOS Host17、factory2、安装1/91.22秒、公共图8/15.08秒通过。
Python Windows20/3/4及macOS35/3/4项通过。本地全量基线、前端/构建/浏览器验收与全分支及定点审查见历史记录。

原生 Windows job及总CI仍为失败：保持原实现的旧共享 LSP Job 回归1通过、1失败，
返回后同一后代句柄仍为258；最终汇总仅有该1组失败。新增native的成功不会抵消旧红灯。
旧算法与严格断言在起点提交已存在，本分支只改过其可见性，未修改Windows运行行为；
未另跑起点SHA的Windows对照，不声称完成独立基线实测或全部回归全绿。
按既有语言不迁移与专用owner范围决定，旧缺陷单列跟踪，不扩展本次四语言交付，也不豁免合并门禁。
Task1–6交付以及Task7的新语言双平台贯通、审查和记录已完成；草稿PR #200和实施票据保持开启等待后续合并处理。
支持包保持 `published:false`，未合并或发布。精确结果和历史失败见[跨平台记录](../research/native-crossplatform-validation.md)。

### 历史实施快照

以下记录各阶段当时的实现与验收状态，最新结论以上述当前状态和跨平台记录为准。

公共 Host、四个原生 helper、应用分派与支持包接入已实现；语言逻辑的独立审查修复完成。
当前剩余项是 Windows 原生 helper 的专用进程所有者及完整平台回归。候选机制的真实三场景、
累计缺口反例和精确1816门禁已通过；共享接入仍被自动审批拒绝，因此改为只接入新增四种 helper，
保留后代能力，既有 LSP/Python 运行行为不变。专用实现的审查和真实平台验收仍待完成。
本机 macOS ARM64 的最终四包已通过生产安装与分析入口验证，同仓库四语言合并测试通过。
全量基线检查为 Rust 751 项、前端 425 项、Node 脚本 180 项通过，Lint、生产构建和真实
浏览器验收通过。最终修复另经 Go 23、Java 22、C# 24、Host 16 与支持包烟测 5 项专项验证。
最终生产源码 `e1b1586` 的四包安装和完整七项真实图验收通过（无跳过）；`42e5a26` 只补充
Java 测试驱动的超时保护，不改变已验收构件。整分支审查及后续定点复核的所有发现已关闭。

经用户授权，已推送实现分支并创建 [草稿 PR #200](https://github.com/XLCYun/narrant/pull/200)，
CI macOS SDK15.5 的完整验收已通过；Windows x64 已修复首跑打包脚本问题，仍待完整成功结果。
`802fadf` 的 Windows 任务到达 120 分钟期限后官方结论为 cancelled；日志确认 LLVM 已下载并
通过摘要校验，但系统 tar 解包一直未完成。`6d1081a` 已确认替换后的完整解包在 87 秒完成，
后续因 VC runtime 分发目录映射错误失败。`4fa0f0d` 已验证目录元数据和全部十个 DLL 版本，
Go/Java/C# 包构建与烟测通过，Clang 链接暴露 LLVM 导出的旧 DIA 绝对路径。
`0e6f8e8` 已修正该路径，Windows 四包构建与烟测均通过；随后 Go 测试的宿主配置假设失败，
`f749fbc` 已作测试数据修正。`2e56a6e` 已通过 Windows Go、Java25、C#29 套件；Clang
Unicode 用例暴露测试驱动使用默认 cp1252 解码，当前改为协议规定的严格 UTF-8。
`360cd26` 已通过 Windows 全部四种语言原生套件，Clang25通过、1平台跳过；Rust Host首次执行
6通过、9失败，Node夹具在初始化随机数时崩溃。修复让测试调用方显式提供生产入口已有的
Windows SystemRoot，保持 Host 清洁环境；本机 Host17项通过。Windows factory、安装和公共图尚未执行。
`d498548` 已确认 Windows Node 启动恢复，Host16项中15通过，剩一项后代进程测试的预期错误
未出现。已修正夹具，使 Windows 后代绕过 Node 自身的随父退出清理并仍由 Rust Job 管理；
本机 Host17项通过，等待 Windows 复验及后续验收。同轮 macOS Host17、安装1、公共图8全流程通过。
`d628bfd` 的 Windows Host16项中14通过，两个按PID检查进程存活的断言失败；正在以稳定句柄
和准确Job归属区分实际回收问题与PID复用。factory、安装、八图仍未执行；Mac全流程继续通过。
新增早期 WindowsJob 探针以缩短诊断，直接调用生产回收实现；本机跨目标编译、两项生命周期
和九项 CI 接线检查通过。生产算法尚未变更，尚无 Windows 四语言完整验收结论。
`76f0204` 的 Windows 短探针已编译，但在 Node 路径校验处失败，尚无 Job 场景结果；
正在修正 CI 入口传递，保留原路径校验和完整验收要求。
`e1f8985` 的实际 Windows 探针已排除本次失败的PID复用及Job逃逸：同一后代句柄尚未退出，
但生产回收返回成功且活跃计数归零。共享 WindowsJob 候选修复尚未应用：自动审批要求
隔离验证及对既有LSP/Python影响的明确授权，当前先补独立机制探针、共享LSP回归及可审查补丁。
`e244845` 的独立零上限探针超时，机制尚未验证；惰性候选审查发现枚举前退出成员的覆盖缺口，
当前继续独立诊断及较窄影响范围的可行性核对，没有应用共享生产修改。
2026-10-03 用户明确授权继续修改共享 Windows 回收逻辑，保持 helper 后代能力；此前审批阻断已解除，
没有采用单进程限制。已补充准确 Job 成员列表与保留句柄的退出窗口诊断，原立即退出断言保持有效。
新的完整性方案通过设计审查：持续保留准确成员句柄，以累计 TotalProcesses 核对捕获覆盖，
全部句柄退出且最终计数对齐才允许成功；通知遗漏或无法确认的对象明确失败。实现及真实 Windows 回归仍待完成。
完成证据审计另外发现原矩阵中的负例未齐：Java 畸形源码与处理器哨兵已补充并通过 25 项测试；
C# 已补齐畸形源码、analyzer/generator 哨兵，并增加明确 net8.0 目标与 net10.0 的 API 隔离和并集对照，
29 项原生测试通过。`85318b7` 的 C# 支持包重新构建及真实四包安装通过；`df62826` 的完整八项
公共图验收通过。生产增量及最终测试修正的定点审查均通过，无剩余发现；`802fadf` 的 macOS
SDK15.5 CI 完整通过（Java25 / C#29、四包安装1、公共图8），Windows CI 仍待真实结果。
GitHub 实施票据 #193–#199 已同步实现进度并链接草稿 PR，保持开启等待平台验收及合并。
未发布支持包或合并主分支；当前不能将 Task 7 或全部目标标记完成。
具体构件版本、摘要与验证范围见 [支持包验收](../research/native-pack-integration.md)、
[真实图验收](../research/native-helper-conformance.md) 和
[跨平台入口](../research/native-crossplatform-validation.md)。

## Global Constraints

- 新语言使用原生 helper 完成语法、语义、源码身份、内容投影及单图并集；Rust 不再做语言 AST 映射。
- 现有 Rust、TypeScript/JavaScript、Python 实现不迁移；共享改动须回归验证。
- 只分析固定侧仓库源码与明确目标标准库；不加载项目第三方缓存，不下载项目依赖，不运行项目构建逻辑。
- 开发工具和 helper 的构建依赖可以下载到隔离工具目录；不修改用户全局工具链，不与项目依赖混淆。
- 对象完整且关系未知可成功；对象不完整、不能正确归并或未知必要配置须失败，不返回部分图。
- 原位读仓库，不复制源码/配置，不写后回滚；工作区释放前所有分析线程和子孙进程必须退出。
- 所有新增方法、类、结构等有易读中文注释；Rust 测试在模块 tests/ 下单独文件。
- Cargo 统一使用 scripts/cargo-agent.sh；开发中聚焦测试，一次最终全量测试，不手动触发 quality-gate。
- 本轮不发布远端支持包、不合并主分支；真实发布需独立授权。可构建和验证本地发布产物。

## 实施约定

工作分为公共协议、四种语言实现、应用/支持包接入、整体验收。独立目录可并行实现，
共享文件由协调者串行整合；所有 Git 暂存和提交由协调者串行执行，避免共享索引冲突。
每个任务提交后做范围内规格与代码质量审查，再做全分支审查。

协议使用 0024 定义的 NDJSON。开发默认值暂定：启动 15 秒、分析 120 秒、取消宽限 2 秒、
退出 5 秒、结果上限 128 MiB、诊断上限 256 KiB；均由 Rust 配置持有，测试可缩短，
不能靠收到日志无限延长。这些为可调整工程参数，不是发布性能承诺。

helper_id / language / profile / projection_revision 首轮固定如下：

| 实现 | helper_id | languages | profiles | projection_revision |
| --- | --- | --- | --- | --- |
| Go | narrant-go | go | go-static-v1 | go-source-v1 |
| Java | narrant-java | java | java-static-v1 | java-source-v1 |
| C# | narrant-csharp | csharp | csharp-static-v1 | csharp-source-v1 |
| C/C++ | narrant-clang | c, cpp | clang-static-v1 | clang-source-v1 |

helper_version 暂定 0.1.0，与首次开发产物一致；正式 pack 版本/目标矩阵由接入与验收冻结。
平台 catalog 的 metadata 每种语言使用显式字段，先在实现中定义并更新 0024a；
不得使用任意 shell 命令、编译器 flag 或项目插件接口代替元数据。

### Task 1: Rust 公共 helper 协议、进程与构图适配

实现 `src-tauri/src/structure_review/native_analyzer/`，并在 structure_review/mod.rs 注册。
遵循 0024 的 ready/analyze/cancel/completed、严格消息校验、输入/来源交叉校验和构图规则。
提供可被四门语言及应用接入复用的 `NativeLanguageAnalyzer`、可执行命令描述、Platform 描述、
HelperIdentity/AnalysisLimits 与取消入口；返回既有 CodeGraph/AnalysisError。
命令来自固定受信工具路径，不能由分析项目提供；env_clear 后仅传显式 runtime 所需环境。
既有 Windows Job/process-group 的实现可抽成公共所有权工具，保持旧 LSP 行为与测试不变。
对 stdout/stderr 有界读取、超时、错误优先、提前 EOF、非零退出、取消、遗留后代、panic/Drop
实现确定清理；不得在成功后等双方 EOF 死锁，不得只杀父进程。
图协议代码和进程代码分模块，避免单文件巨型实现。
测试用独立假 helper 真实子进程覆盖 P1/P2/P3、G1、S1/S3，不能只 mock 解码。
聚焦 Rust 测试经 cargo-agent；最终全量测试交给 Task 7。
所有权：native_analyzer/、structure_review/mod.rs 及必要最小共享进程提取。
不要修改 application、language_support、前端或 Cargo 依赖，必要新增依赖先向协调者说明。

实际交付说明：Windows使用native专用所有者，跟踪器只有一份；为限制影响范围，有限复制悬挂启动原语。
旧LSP/Python所有者没有迁移，共享算法的已知次序缺陷另列于上述当前状态。

### Task 2: Go 原生 helper 与受限静态项目加载

实现 `native-helpers/go/`，独立 Go module、构建说明、测试及 fixtures。
使用 go/parser、go/ast、go/types 和只读源码 importer；不依赖默认 go list/go/packages driver。
在已有探针基础上实现真实 go.mod/go.work 的有限静态模型：仓内 modules/use/replace、
明确目标 SDK、平台文件/build tags 选择、仓内包映射、依赖拒绝和标准库加载。
不能把探针手工包映射或两平台写死当生产发现；未知必要配置显式失败。
自动发现明确平台文件/静态目标集合并分析，任意自定义 tag 无证据时报告不支持范围。
实现六类高层对象、跨文件类型/调用关系、稳定 ID、own-content、tests 范围、并集和原子错误。
覆盖函数/方法/类型/字段/全局值、init 顺序、多名初始化、泛型、匿名表达式归 owner；
不因函数体错误就静默丢引用；interface 调用保留已确认静态接口目标与未知动态实现的区别。
真实协议入口和取消读取线程；输出与公共 v1 一致，正常终结后自行退出。
验证至少：真实 module/workspace、两配置关系并集、目标标准库、缺第三方、只读路径/字节、
同名不同声明、重复/非法输入、source/child/body-only diff 所需投影，以及取消退出。
可用固定 Go 1.23.x 作为首个本机验证版本，明确记录兼容边界；不能声称未测版本支持。
所有权：native-helpers/go/，语言实现验收记录 `docs/research/go-native-implementation.md`。
不修改共享协议、Cargo、应用和 pack 清单；接口冲突及时回报。

### Task 3: Java 原生 helper 与目标隔离验证

实现 `native-helpers/java/`，固定 JDT Core 构建依赖、可重复构建、fixtures 与测试。
先获取隔离开发 JDK/JDT（可下载官方发布工具到 .tools 或 /tmp，不做全局安装），执行目标
API 隔离与 recovered binding 探针，选择单一生产后端；如 JDT 无法满足允许范围，记录证据后
改选 javac，不做运行时双后端自动回退。
实现 java-static-v1：明确 source roots/release 与可证明的有限 POM 模型；不执行 Maven/Gradle。
对于 Gradle/未知必要动态配置，返回明确不支持；允许本侧已有、schema 固定的静态分析元数据，
但不能要求所有普通支持项目都先运行导出器。目标平台匹配，运行 JDK 不作为目标 API 默认值。
实现类型、方法/构造器、字段/枚举/record组件、初始化块、局部/匿名类型、package/import
映射与规范内容，隐式生成成员不伪造成源码节点；绑定有证据才 Node/External，recovered 为未知。
实现 v1 协议、取消、配置并集、源码来源 evidence，与 Go/Rust 共用相同消息语义。
验证基础项目、跨文件重载、旧目标API反例、缺第三方、body/child/格式变化、include_tests、
无处理器执行、只读、超时及取消。JPMS 等无法可靠支持时显式拒绝，保留待扩展矩阵。
所有权：native-helpers/java/、docs/research/java-native-implementation.md。
不修改共享文件，不提交；构建依赖安装与项目依赖解析严格分开。

### Task 4: C# Roslyn helper 与静态项目模型

实现 `native-helpers/csharp/`，固定 Roslyn 与 .NET SDK/runtime 版本、可重复构建、测试。
使用隔离工具目录，官方 SDK 可下载用于开发；不全局安装、不运行被分析项目构建。
直接 CSharpCompilation 与目标 reference assemblies；不使用 MSBuildWorkspace。
csharp-static-v1 支持明确 source-set 与有限普通 SDK-style csproj（字面 TFMs、Compile、
ProjectReference、已验证等值条件）；默认 glob/隐式常量/usings 必须按固定规则重现，
未知 imports/targets/属性函数等影响范围则失败。仓内项目引用可作上下文，排除 NuGet业务缓存。
实现 0024a 对象、partial片段/成员身份、签名/own-content、accessor/record/顶层语句、
调用/继承/实现/方法组关系，匿名表达式保持原文归 owner；不能挑 partial 第一个充当全部。
v1 协议完整，多个明确配置各自分析后合并，无配置UI；无法表达冲突则完整失败。
验证目标TFM隔离、跨文件/partial/重载、条件源码并集、缺依赖、无生成器执行、只读、
格式/child/body-only内容与取消。未支持 Razor/Unity等须清楚拒绝，不声称广泛兼容。
所有权：native-helpers/csharp/、docs/research/csharp-native-implementation.md。

### Task 5: C/C++ LibTooling helper 与编译上下文

实现 `native-helpers/clang/`，固定 LLVM/Clang 构建、CMake或明确构建脚本、fixtures与测试。
采用真实 LibTooling API，不以 AST JSON dump 作为生产协议；开发可准备隔离官方LLVM工具。
clang-static-v1 读取本侧 compilation database 的明确参数，保留不同TU配置；仅解释受控数据，
不运行 command/wrapper/query-driver/CMake。校验response files、include/sysroot和参数来源，
拒绝插件/模块自动构建等执行/写入入口。目标headers来自catalog，业务外部headers不载入。
分别支持C/C++模式，初始C17/C++20验收。源码范围/宏/USR只作为证据，生产身份基于稳定源声明；
原始源码投影避免推导类型伪冲突，冲突类别/父级按Q5失败。prototype/definition、模板声明与
实例、匿名实体、包含和多header上下文需要明确映射，不按同名或链接名覆盖。
真实v1协议与取消/限额。验证已有5种并集fixture、跨TU/header、重载、缺header完整性、
动态调用未知、C/C++差异、参数拒绝、只读与退出。不能只通过探针便宣布实现完成。
所有权：native-helpers/clang/、docs/research/clang-native-implementation.md。

### Task 6: 应用分派、支持包和构建接入

在Task1及各helper接口确定后接入四种新语言分组、支持包识别、目标环境发现和标准错误路径。
扩展pack model/catalog/build脚本与固定manifest验证；现有语言包保持行为，缺新包不能被当成
unsupported-language静默忽略。支持包包含helper执行runtime，项目目标SDK按验证范围发现。
构建/烟测可用本地产物完成；不能伪造远端已存在版本、摘要或发布成功。
UI只增加语言/包名称和必要错误信息，不增加配置切换；如改交互布局须真实浏览器验收。
覆盖相应Rust、Node和前端测试；分派完整图、缺工具失败、两侧取消及旧语言回归。
所有权：application、language_support、tauri装配、resources、scripts、package/i18n相关接入。

### Task 7: 贯通验收与全分支审查

将真实四helper通过Rust适配器运行合成多文件/多语言前后侧比较，验证完整CodeGraph与diff，
不是仅逐后端CLI输出；固定源工作区、同语义两目录ID、内容/关系/结构差异、未解析关系、
目标隔离、取消、进程树、只读、支持包入口均有证据。
按已承诺macOS ARM64和Windows x64分别验证；本机不能覆盖的目标记录未完成，不伪称通过。
聚焦检查通过后执行一次Rust全量测试、所需Node/前端类型/构建检查；不手动quality-gate。
独立全分支审查并修复重要问题。更新实现规格、验收记录及GitHub实施Issue状态。
完成审核时逐项检查本计划所有任务与0024验收，不以部分语言/协议或文档完成替代最终目标。

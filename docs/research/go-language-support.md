# Go Language Support 调研

后续工程草案：[公共进程协议](../specs/0024-native-analyzer-helper-protocol.md)、[语言配置与验收](../specs/0024a-native-language-analysis-profiles.md)。本文保留调研过程中的候选和历史待决项；已确认决策以 ADR-0034 为准，工程建议以草案和后续实测修正为准，未验收项不构成发布承诺。

最新加载实验见本文第 15 节及[可复跑探针](probes/go-native-helper/README.md)：下一轮优先验证受限静态项目模型与源码 importer；默认 go/packages driver 的只读阻断仍未消除。

共同输出语义已由 [ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 确认：helper 返回单张配置并集图，对象和已解析关系至少在一种已分析配置下成立，不保留配置归属，不新增配置切换/变体界面。必要的配置分析与兼容源码对象归并在 helper 内完成；无法正确归并按完整图契约失败。下文具体发现算法与后端能力仍须验证，不能把并集理解为穷举所有配置或直接同时启用互斥源码。

研究日期：2026-10-02。仓库基线：`6761aca`。状态：方案建议，尚未成为 Go 实现规格；后续已决架构与范围见下。

> **后续已决范围：** [ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 确认 Java、Go、C#、C/C++ 各自使用原生 helper，在同一进程中取得语法与语义，通过统一接口返回规范化的 CodeGraph 构造 DTO。helper 负责语言特有类别、自身内容投影、稳定身份、图成员及 Node / External / Unresolved 关系分类；Rust 负责输入来源边界、协议、通用校验、最终 CodeGraph 构造与生命周期。跨进程传输不采用整个编译器 AST。
>
> 四门新语言均不执行被分析仓库及其依赖携带的构建脚本、插件、注解处理器或生成器；受控分析工具也不得借加载触发这些行为。默认输入为仓库源码与明确目标的标准库/平台声明，不为分析下载或 restore 第三方，也不自动纳入本机已有第三方缓存或环境；仓库已提交源码按项目和选集规则处理。Rust、TS/JS、Python 现状不变，Rust 可选的 `all-dependencies` 保留。具体字段协议、目标支持矩阵及后端版本仍待设计和验证；下文超出此范围的原研究候选不属于当前实施路线。

本文标记 **事实**（官方资料或当前代码可直接证实）、**实测**（本次实际执行）、**推断**（由事实推出的限制）、**建议**（需要实施的设计）和 **未验证**（仍需实验）。本文只调研 Go；没有修改业务代码、创建 Issue/PR、执行 Cargo 或触发 quality-gate。

## 1. 结论与版本边界

**建议：在已决的原生 helper 架构下，首选预编译 Go helper，以 `go/parser` + `go/ast` 提取声明和自身内容，以 `go/packages` + `go/types` 提供项目、类型和符号证据；gopls LSP 保留为原研究候选及交叉验证对象。** helper 的 Go runtime 随二进制提供；项目 SDK 单独发现、校验并冻结，不能因为已安装支持包便认为项目工具链可用。默认输入限于仓库源码与明确目标标准库/平台声明，不运行仓库及依赖携带的构建脚本、插件或生成器，不自动安装工具链或项目依赖，也不纳入本机已有第三方缓存；对象提取完整时，缺失依赖使受影响关系保持 Unresolved；无法保证完整性则不能作为普通成功结果，工具缺失、非法输入、协议失败和取消仍是硬错误。**实测发现 stock `go list -mod=readonly` 仍会写 go.work.sum；首选路线以解决原位只读加载为发布前提，不能直接包装默认 driver 上线。**

**事实：当前来源观察到 Go 1.27.1、x/tools v0.50.0、gopls v0.23.0。** 这些是本次页面/标签的可核实版本，不能等同“所有平台的最新补丁已验证”。x/tools v0.50.0 与 gopls v0.23.0 的 `go.mod` 均要求 Go 1.26.0；这与分析较早语言版本的源码是不同问题。正式支持包必须另选并锁定一组完成兼容测试的版本、源码提交和摘要，不能直接使用 `latest`。[Go 发行归档](https://go.dev/dl/)、[x/tools v0.50.0 go.mod](https://raw.githubusercontent.com/golang/tools/v0.50.0/go.mod)、[gopls v0.23.0 发布说明](https://go.dev/gopls/release/v0.23.0)、[gopls v0.23.0 go.mod](https://raw.githubusercontent.com/golang/tools/gopls/v0.23.0/gopls/go.mod)。

**事实：本机工具与上述候选版本不同。** 本次只读探测得到 `/usr/local/go/bin/go` 为 Go 1.23.8、`/Users/lizhanfa/go/bin/gopls` 为 v0.22.0，后者构建信息为 Go 1.26.3，主机为 Darwin arm64。不能用本机的旧 SDK 实验替代 Go 1.27 或 x/tools v0.50.0 的验收。

## 2. 当前 Narrant 契约与真正可复用的部分

本节依据当前实现，而非仅沿用历史研究。已读取 `AGENTS.md`、`CONTEXT.md`、domain 文档、0018/0019/0021/0022/0023 规格和 ADR-0030/0031/0032/0033。

| 当前事实 | 对 Go 的约束 |
| --- | --- |
| [`LanguageAnalyzer`](../../src-tauri/src/structure_review/code_graph/analyzer.rs) 是同步 `analyze(&AnalysisInput) -> Result<CodeGraph, AnalysisError>`；公共选项只有 `include_tests` | Go 的 SDK、tags、平台、依赖范围应是 adapter 配置快照，不把 Go 特有概念塞入公共输入 |
| [`CodeNode`](../../src-tauri/src/structure_review/code_graph/node.rs) 有六类，`native_kind` 保留原生差异；`signature`、`code` 和语义 extensions 参与比较，`source` 不参与 | Go 的泛型、接收者、约束、struct tag、构建条件必须有明确归属；不能把诊断、绝对位置写入比较字段 |
| [`CodeGraph` 验证](../../src-tauri/src/structure_review/code_graph/validation.rs) 要求稳定 ID、单父无环 containment、有效 Node target；references 按事实集合去重 | 未选中的同仓库目标是 Repository External，不能因能加载就增加 Node；导出符号也不自动进入图 |
| [`Rust`](../../src-tauri/src/structure_review/rust_analyzer/mod.rs) 使用本地语法 + rust-analyzer，可选择依赖范围；[`TS/JS`](../../src-tauri/src/structure_review/typescript_analyzer/mod.rs) 使用 Oxc + 固定语言服务；[`Python`](../../src-tauri/src/structure_review/python_analyzer/mod.rs) 使用语法抽取 + ty | 现有三门实现保持不变，Rust 可选 all-dependencies 保留；Go 的 helper 与默认输入边界按 ADR-0034 独立确认，不能从既有后端技术路线推导 |
| [`application`](../../src-tauri/src/structure_review/application/mod.rs)、[`workspace`](../../src-tauri/src/structure_review/application/workspace.rs) 准备固定两侧、并行分析、失败取消同组并等待所有 worker | Go helper 必须在 workspace 回调结束前回收进程树；不得持有工作区到后台继续分析 |
| [`language_support`](../../src-tauri/src/language_support/model.rs) 和[包目录](../../resources/language-support-packs.json) 锁定支持包版本 | 加 Go 不止扩展文件后缀，还必须完成包身份、下载、校验、租用与释放路径 |

**ADR 边界。** [ADR-0030](../adr/0030-use-extensible-code-object-kinds.md) 允许 Go 原生种类而无需扩大六类枚举。[ADR-0031](../adr/0031-reuse-review-head-for-structure-analysis.md) 禁止 adapter 复制源码、配置或声明文件，允许 helper/control metadata 和内存配置；因此复制模块到临时 GOPATH、重写 `go.mod`、生成临时源码 overlay 文件均不是现成可接受方案。[ADR-0032](../adr/0032-separate-language-support-packs-from-project-toolchains.md) 区分包与项目工具链；自动运行 `go install`、`go get`、工具链下载或替用户改项目 SDK 需要新决定。[ADR-0033](../adr/0033-bound-python-analysis-to-repository-and-standard-library.md) **仅决定 Python**；Go 默认输入及执行边界由后续 [ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 独立确认，目标标准库版本矩阵仍待验证。

**当前文档与实现用词差异。** [0018b](../specs/0018b-code-relationships.md) 推荐表写 `code.inherits`，当前 TS/Python 使用 `code.extends`；实现还使用 `code.references`、`code.type_use`。Go 首版无需凭空建立继承边。`code.reads`/`code.writes` 当前并未实现，不能把预留词汇写成 Go 必须完成的现有能力。

## 3. 候选后端

| 路线 | 事实与长处 | 代价、边界 | 判断 |
| --- | --- | --- | --- |
| `go/parser` + `go/ast` | 官方 AST；`ParseFile` 可读取原文件或已固定的内存字节；可识别声明、注释和局部结构 | 无项目导入/类型绑定；parser 接受部分超出合法语言的结构，成功 parse 不等于程序有效；`ParseDir` 不处理构建标签，已被弃用 | 作为所有模式的语法层；不能单独宣称完整语义。 [parser](https://pkg.go.dev/go/parser) |
| AST + 直接 `go/types.Config.Check` + 自建 importer | 可控制每个输入文件、导入边界和错误，避免语言服务往返 | 要自行重建 package discovery、模块解析、vendor、go.work、构建约束、标准库导入及导出数据兼容；`go/importer` 不负责模块感知的项目加载 | 可作无项目语法探针和降级实验；不建议首版重写 Go 构建系统。 [go/types](https://pkg.go.dev/go/types)、[go/importer](https://pkg.go.dev/go/importer) |
| **AST + `go/packages` + `go/types` helper** | 同一进程直接获得 AST、Defs/Uses、Selections、Instances、包元数据；能一次遍历计算完整选集图 | 默认 driver 使用 `go` 命令；`go list -export` 可能触发编译，仍需要兼容 SDK 和受控缓存；Narrant 要维护小型协议与进程生命周期 | **首选**。公开 API 足够，避免依赖 gopls internal。 [packages API](https://pkg.go.dev/golang.org/x/tools/go/packages)、[v0.50.0 golist 源码](https://raw.githubusercontent.com/golang/tools/v0.50.0/go/packages/golist.go) |
| AST + gopls LSP | 官方语言服务、已有类型检查和索引；可用 Narrant LSP 请求/取消基础设施 | 仍依赖项目 go；需要独立 AST 获取自身内容；请求驱动，不是全图 API；全局实现查询的语义不完全等同确定实现关系 | 原研究备选，当前 helper 路线不采用；保留对照 definition 等少量结果的价值。CLI 不是稳定集成接口。 [gopls navigation](https://go.dev/gopls/features/navigation)、[CLI 稳定性说明](https://go.dev/gopls/release/v0.23.0#configuration-changes) |
| Tree-sitter-Go 或其它第三方语法树 + 上述语义层 | 可在 Rust 侧直接解析，有增量语法优势 | 仍需第二套 Go 类型与项目加载，增加语法版本一致性风险；没有证据表明其对本任务优于官方 parser | 暂不引入；本次未实测，不作为已验证备选 |

**不把 LSP 结果直接当最终图的关键证据：** gopls call hierarchy 不包含动态调用，也不把嵌套函数当独立层级；type hierarchy 只覆盖命名类型，别名和局部类型有边界。[官方导航说明](https://go.dev/gopls/features/navigation#call-hierarchy)。更进一步，已读取的 gopls `master` 实现明确注明：泛型实现查询可能只代表“某种实例化可实现”，约束接口的非接口 type terms 会被忽略并可能产生假匹配。该源码是**滚动分支观察**，本次未成功获取 v0.23.0 对应文件，不能声称固定版一定相同；固定候选版必须再核对。[implementation.go](https://raw.githubusercontent.com/golang/tools/master/gopls/internal/golang/implementation.go)。

**建议的 helper 分层：** `project` 在 Rust 确认的输入来源边界内解释项目/构建 profile；`syntax` 负责 AST/自身内容；`identity` 管理稳定 Narrant ID；`semantic` 消费类型检查结果；`graph` 确定成员及 Node / External / Unresolved 分类并输出规范化 CodeGraph 构造 DTO。Rust 负责协议、通用校验、最终图构造与生命周期，具体字段与编码仍待设计。不要直接把 `types.Object` 地址、`packages.Package.ID` 或 token.Pos 当持久身份。`packages.Package.ID` 是 driver 定义的 opaque 值；`objectpath` 可标识包 API 中可达对象，但不保证局部变量/局部类型都可编码，不能替代 Narrant 自有身份协议。[packages 源码](https://raw.githubusercontent.com/golang/tools/v0.50.0/go/packages/packages.go)、[objectpath](https://pkg.go.dev/golang.org/x/tools/go/types/objectpath)。

## 4. 六类 Code Object 映射建议

以下是 **建议**，没有将 Rust/TS/Python 的个别取舍当作 Go 的既成产品决定。

| Code Object 类别 | Go 映射及 native_kind | 自身内容、归属 |
| --- | --- | --- |
| Container | 已选文件：`file`；模块/包元数据用于身份与解析 | 每文件一个物理容器，`name` 为仓库相对路径；建议 Go file 的 signature 保留 package clause、import 路径/别名、生效 build/directive 与 cgo preamble；其 code 只保留初始化声明有序锚点，不重复子内容。source 仅展示。仅 package/import 的合法文件也保留这些内容；完全空 `.go` 缺 package clause，不能当合法空图。该 Go 投影不同于当前 TS file 空签名约定，实施规格需明确 |
| Type | `struct`、`interface`、`defined_type`、`type_alias`；函数体中的具名 `type` 声明同样可提取 | type header 保留类型参数及约束；struct/interface 的独立字段/方法从父自身内容剔除，以稳定锚点保持顺序；匿名 struct/interface 先保留在拥有者内容中 |
| Callable | 包函数 `function`、接收者方法 `method`、接口显式方法 `interface_method`、`init_function` | 函数签名保留 receiver、type params、参数、返回值；实现为去掉单独子声明后的函数体。方法物理上仍在声明文件，`code.member_of` 指向接收者 Type，避免跨文件单父冲突 |
| Value | 包级 `var`/`const`、struct 字段/嵌入字段、局部显式 `var`/`const`（按首版范围决定） | 单独初始化表达式属于 Value；`const` 继承表达式与 `iota` 序号需显式规范化，否则插入条目可能改变值而不改变原文字面；多名共享初始化归一个 ExecutionRegion，Value 只引用该声明锚点 |
| ExecutionRegion | `var a,b=f()` 的共享初始化：`declaration_initialization` | 一组共享 RHS 一个 owner，calls 只出现于该 owner；file.code 保存它与其它初始化声明的有序锚点。Go 没有一般模块级语句；不伪造 Python 式 module body，`init` 仍是显式 Callable |
| Extension | 首版没有必需实体 | `//go:build`、`//go:embed`、`//go:linkname` 等先是所属对象的 Go 语义 metadata/内容；没有证据时不生成跨语言调用或 synthetic 函数 |

**事实依据：** Go 的包块、文件块、具名声明及 `init` 特殊绑定规则不同；`init` 可重复而不能被普通名字引用；struct 的嵌入并非类继承。[Go 声明与作用域](https://go.dev/ref/spec#Declarations_and_scope)、[包初始化](https://go.dev/ref/spec#Package_initialization)、[struct 类型](https://go.dev/ref/spec#Struct_types)。

**闭包建议：首版作为表达式保留在最近独立 Code Object 的 `code`，其关系归属于该拥有者。** Go 函数字面量会捕获外层变量，但为每个匿名 literal 建独立 Callable 会立即引入匿名身份和祖先内容剔除的产品取舍。若用户希望独立比较闭包，后续增加 `function_literal` Callable，并使用拥有者 + 语义槽位/匿名序号，先完成重排稳定性实验；立即调用字面量也可准确指向该节点。不要因 gopls 把闭包合并而无声丢弃其调用，也不要把 function literal 当作具名嵌套 `func` 声明。[函数字面量](https://go.dev/ref/spec#Function_literals)。

## 5. 当前 Code Relationship 的映射

| 关系 | 建议提取规则 | 不应生成的“确定事实” |
| --- | --- | --- |
| containment | 文件 → 包级声明；type → 自身显式字段/接口方法；Callable → 被单独建模的局部声明 | 方法因 receiver 在另一文件而被复制到两个父节点；未选依赖对象混入 containment |
| `code.calls` | AST `CallExpr` + `Info.Uses/Selections` 确认具名函数或具体接收者方法；泛型调用归一到原声明；`go f()`/`defer f()`同样观察真实调用目标 | 把类型转换 `T(x)`、`make/new` 类型参数或 `f` 的同名变量当普通函数；通过接口/函数值动态调用推断唯一实现 |
| `code.member_of` | 接收者方法 → 声明的具名 receiver Type；字段/接口方法 → 声明所属 type | promoted method 复制为新声明；嵌入导致多个物理所有者 |
| `code.implements` | 首版仅发出类型完整、由 `types.Implements` 证明的非泛型声明关系；只枚举已加载候选，界面不声称全仓所有可能实现 | 仅同名方法、仅 gopls implementation 回答、泛型“可能存在实例化”、将某个具体实例的证明泛化到原声明、未知 imported interface 或所有类型→`any` 的海量边 |
| `code.imports` | 文件拥有 import；解析到真实包身份后为 External 包符号，别名/`.`/`_` 均保留导入事实 | 看见 import 字符串便声称包已可用；`_` 导入不等于对某个 `init` 的显式调用 |
| `code.references` | 首版限定已确认的函数/方法值非调用使用；方法值 `x.M` 引用 M，函数值 `f := target` 引用 target；普通数据引用省略 | `f()` 一定调用最近曾赋给它的函数；把普通数据引用扩为已决定范围；反射、plugin、unsafe、linkname 的字符串目标自动连边 |
| `code.type_use` | 首版按当前提取约定限定字段/嵌入字段类型；签名、receiver、断言/转换、泛型约束等广义 type use 是后续扩展建议 | 将新范围说成当前共有语义；把泛型 type-set term 当继承或普通运行时接口实现 |
| `code.applies_to` | Go 首版没有 Rust impl / extension block 的对应实体，通常不发出 | 用虚拟 impl 包装 Go 方法来满足类别 |
| `code.extends` / `code.inherits` | struct embedding 使用 field + type_use；接口 embedding 保留在 Type 自身内容，独立关系另定语义 | 将 Go embedding 等同类继承；未经共同语义决定就选两种历史拼写之一 |
| `code.reads` / `code.writes` | 首版不做，保持与当前实现范围一致 | 仅靠标识符出现位置宣称读写分析已完成 |

**事实：** `go/types.Info` 分别提供 Defs、Uses、Selections、Instances，能区分选中字段/方法与普通包限定符；有类型错误时信息可能不完整。不能把一个非 nil 的 Info 当“全包已确定”的证明。[Go 1.27.1 types API 源码](https://raw.githubusercontent.com/golang/go/go1.27.1/src/go/types/api.go)。

**接口与方法集。** 同时检查 `T` 和 `*T` 的方法集，并保留接收者形式的区别。不能只看到指针方法就发出 `T implements I`。建议当只有 `*T` 满足时，暂缓该边，或在新 Go 规格中明确扩展引用 `go.pointer_implements`；共享 `code.implements` 若只能指向一个 T 节点会丢失 receiver 语义，需要先明确约定。隐式实现不是声明继承，接口 type set 与泛型约束还需要 `types.Satisfies` 等与具体实例一致的判断。[Method sets](https://go.dev/ref/spec#Method_sets)、[types.Implements/Satisfies](https://pkg.go.dev/go/types#Implements)。

**泛型关系的表示限制。** `types.Implements(Box[int], I)` 即使成立，也不能在把实例归一为 `Box` 后发出“所有 Box 都实现 I”的边。当前 CodeReference 没有类型实参或条件 payload，而本建议不创建实例节点，所以首版将这类证明留作内部证据，暂不输出无条件 `code.implements`。以后若要展示，应先定义保留实例条件的关系契约。跨两次 `packages.Load` 的类型对象也不能混做 Implements/Identical；它们属于不同类型域。[packages.Load 类型身份约定](https://raw.githubusercontent.com/golang/tools/v0.50.0/go/packages/packages.go)。

**动态调用的推荐保守语义。** `i.M()` 若只能确定接口方法，先保留 `code.references` 到接口方法，以及 `code.calls → Unresolved("i.M")`，不要连到所有实现或猜测具体对象。将 `code.calls` 定义为“静态接口槽位”也是可行备选，但需要产品/跨语言语义明确，否则 UI 容易把它读成确定实现调用。`f := target; f()` 的第二步仍先保留 Unresolved；方法表达式 `T.M(x)` 可以用类型检查证据解析；直接函数引用的泛型实例由 `Origin()` 归一到声明。更精确的函数值流分析需要单独的 SSA/数据流阶段，不能把 CHA/RTA 的候选集包装成唯一事实。[SSA CallCommon 与 StaticCallee](https://pkg.go.dev/golang.org/x/tools/go/ssa#CallCommon)、[Func.Origin](https://pkg.go.dev/go/types#Func.Origin)。

## 6. 身份、重载、泛型、生成代码与自身内容

**建议身份示意：** `go:repo:<module-relative-dir>:<package-relative-dir>:<logical-package-kind>:<declaration-file>#<owner-chain>/<kind>:<name>`。logical-package-kind 只由源码决定（normal 或 external_test），不使用 driver 的普通/测试增强变体 ID，切换 include_tests 不改变生产声明身份。字段和方法增加类型拥有者；绝不放入 checkout 前缀、cache 路径、token.Pos、类型检查对象地址或 SDK 安装目录。这里是编码示意，实施时需要无歧义 escaping 和 golden tests。

- Go 普通函数没有按参数签名区分的同作用域重载；方法用 receiver 类型区分。签名变更应保留节点身份，不要把所有参数文本放进 ID。`init` 和 `_` 不形成普通绑定，需要“声明文件 + 同类声明槽位”区分；匿名/重复槽位重排可能成为删除/新增，须公开保守边界。[声明规则](https://go.dev/ref/spec#Declarations_and_scope)。
- 模块路径与包名分别保存；`a/logger` 和 `b/logger` 可都叫 `log`，包子句和导入别名都不能充当唯一 ID。同一个目录的正常包与外部 `p_test` 是不同语义包；测试加强版中的同一个生产声明仍只输出一次。
- 泛型声明建一个节点，类型参数/约束进入 signature；`F[int]`、`Box[string]` 不新建“编译后实例”节点，关系目标回到 Origin。显式 type argument 留在调用者自身内容；泛型 alias 保留 `type_alias`，不能因为可解到底层类型便消灭源码声明。[Info.Instances](https://pkg.go.dev/go/types#Info)、[Alias.Origin](https://pkg.go.dev/go/types#Alias.Origin)。
- 局部 type/显式局部声明使用最近建模 owner + 词法作用域槽位 + 名字。普通 `:=` 临时变量、参数/结果变量先不单独生成 Value；它们仍用于正确绑定与 shadowing。文件移动/声明改名不做推断匹配，与共同契约一致。
- 标准库 External 用 `go:stdlib:<package>:<object>`，实际 SDK 版本进入分析配置/cache key；已确认的 builtin 用专门 `go:builtin:<name>`。模块路径、已解析版本/仓库内 replace 身份、包路径与稳定对象路径可辅助区分仓库中的上下文声明；原研究的仓库外业务依赖 External 不属于当前默认输入。不要把 package.Name 当外部符号 ID。Vendor 是否算 Repository scope 按实际来源边界决定，见下一节。
- `objectpath` 可用于已导出 API 的 External 辅助编码，但其对局部对象、export data 裁剪对象的限制要求 fallback；不能使用 “编码失败 → 按裸名 External”。[objectpath API](https://pkg.go.dev/golang.org/x/tools/go/types/objectpath#For)。
- 源码比较建议使用 Go token 序列与稳定子声明锚点；忽略普通注释/排版，但保留字面量、操作符、显式/隐式分号语义、struct tag 和生效的 `//go:` 指令。`go/format` 可用于可读签名，但其输出可能随 Go 版本变化，必须随 helper 固定版本。[go/scanner](https://pkg.go.dev/go/scanner)、[go/format 的版本说明](https://pkg.go.dev/go/format)。
- `const (A=iota; B)` 的 B 原文没写表达式，前项变化会影响它；需要保留继承的表达式、显式类型和本组 iota 序号，不能只比较 B 的空 RHS。可靠常量值可辅助验证，但缺依赖时不能伪造求值结果。`var a,b=f()` 只发生一次调用，建议固定一个 declaration_initialization ExecutionRegion 拥有 RHS，各 binding 仅声明，不能复制两次 calls。
- struct 字段顺序影响布局与无键复合字面量，不能因 `contains` 比较按集合就丢弃次序。建议在 Type 自身 `code` 的有序锚点中保留字段次序；字段实现内容仍只归子节点。初始化与 const group 的有序投影同理，不能直接照搬其它语言“删除所有成员后仅留外壳”的规则。
- `var A=f(); var B=g()` 的声明顺序变化也必须可见；file.code 或明确的初始化区域保留有序锚点，不能只依赖各 Value 自身 code 和无序 contains。顺序正确性是首版前提，只有是否独立展示执行区域属于产品粒度选择。
- tracked 生成 `.go` 只要被选中就正常提取；`ast.IsGenerated` 可做来源标记，不默认排除。未提交生成文件不由 Narrant 运行生成器补齐。`go list` 产生的 cgo/SWIG/cache 文件是语义中间材料，不得变成选集对象或用临时路径生成身份。[ast.IsGenerated](https://pkg.go.dev/go/ast#IsGenerated)、[go list -compiled](https://pkg.go.dev/cmd/go#hdr-List_packages_or_modules)。
- `//line` 可改变诊断坐标；身份/切片必须使用 token.File 原始 Offset 与未调整位置，并确认路径仍属于真实输入，不能让 line directive 把仓库声明伪装成外部文件。[token.File.PositionFor](https://pkg.go.dev/go/token#File.PositionFor)。
- 仓库路径编码统一使用 `/` 并转义分隔符；Unicode 名称与路径不得退化为 lossy 编码。helper 传输物理字节范围，Rust 校验 UTF-8 边界；原 gopls 候选的对照实验另可用现有 [SourceText](../../src-tauri/src/structure_review/source_text.rs) 校验 UTF-16/字节转换。中文、非 BMP、CRLF、同一行多个声明和 Windows 路径须有对应负例，不能用展示行号代替来源证明。

## 7. 文件选集、项目上下文与解析边界

**建议：选集只决定产图；语义加载单位仍是包。** 同包未选文件必须参与类型检查，否则会漏掉合法本地函数、方法和类型；只有仓库源码及明确目标标准库/平台声明内的导入包可作为上下文，输出只包含调用者选中的已支持源码。优先按仓库内部 module/workspace 分组，以 `file=<absolute-selected-file>` 或受控 package patterns 加载所需根包；file query 不意味着只分析该文件。两阶段可先查 metadata 再按根包执行 `LoadSyntax`，不要无条件 `LoadAllSyntax ./...`。[packages queries](https://pkg.go.dev/golang.org/x/tools/go/packages)。

**建议 load mode：** 根包取得 `NeedName | NeedFiles | NeedCompiledGoFiles | NeedImports | NeedTypes | NeedSyntax | NeedTypesInfo | NeedTypesSizes | NeedModule`，测试需要相应 ForTest 数据；只在允许输入范围内按需扩展上下文语法。`NeedDeps` 与 `NeedSyntax` 同用会把依赖解析范围扩大，必须先约束来源再以实测预算决定。不同 `packages.Load` 的类型对象不能按指针直接混用；需要在一次加载中比较类型，跨组通过稳定身份合并图。

| 观察 | 输出分类建议 |
| --- | --- |
| 目标声明已确认且在已选提取节点索引中 | `Node` |
| 目标声明已确认且 canonical 源码位置在该侧固定仓库内，但未被选中 | `External` + `SymbolScope::Repository` |
| 明确目标的真实标准库/平台声明或预声明目标 | `External` + `SymbolScope::Dependency` |
| 仓库外第三方 module cache 或其他本机业务依赖 | 不加载；相关引用缺少允许范围内的证据时为 `Unresolved` |
| tracked vendor 包位于仓库内 | 物理 scope 为 Repository，额外身份保留 module/version/vendor 来源；“第三方”来源与 scope 是两个维度，不能悄悄改现有 Repository 的含义 |
| import 缺失、函数值/接口动态派发不明、配置排除导致未知、C 符号未建模 | `Unresolved`；仅保留观测名称，不伪造包/函数 External |
| 在仓库外的本地 `replace` 或 `go.work use` 指向可变源码 | 默认阻止读取/拒绝该 project profile；不要因为同一作者的目录便视作固定仓库上下文 |

被测试选项排除的声明不能经 External 边重新显露；目标匹配应在测试过滤之后检查成员及资格。标准库来源须有选定 GOROOT 或 `go list` 的实际标准库证据，不能把 `Module == nil` 一概解释为标准库（GOPATH 项目也可能没有 module）。builtin 则来自预声明类型域；其专门身份不需要制造源码节点。

**关键限制：仅 canonicalize 最终 target 不足以限制读取。** 默认 go/packages/go list 在返回路径前就可能读取外部 replace/use、调用工具、写 sum/cache；在加载前预解析 go.mod/go.work、校验本地路径、固定环境、控制 driver 才有意义。`GOPACKAGESDRIVER=off` 防止继承环境带来的自定义 driver；Bazel 等构建系统若需要执行仓库或依赖携带的脚本/插件，其导入路线已排除。gopls 也没有自动解决该隔离问题，官方目前只把 Go command 作为正式支持的构建系统。[packages driver protocol](https://pkg.go.dev/golang.org/x/tools/go/packages#hdr-The_driver_protocol)、[gopls 当前设计注记](https://go.dev/gopls/design/design)。

**事实：`Config.Overlay` 不能被当成“只在内存替换”的捷径。** x/tools v0.50.0 默认 driver 调用 `gocommand.WriteOverlays`，该实现会建立临时文件及替换映射；用它复制选中源码、go.mod 或 go.work 与 ADR-0031 冲突。选中源码已在固定 workspace，无需 editor overlay；如果要改变加载配置，必须使用真正不复制源码/配置的设计。[packages defaultDriver](https://raw.githubusercontent.com/golang/tools/v0.50.0/go/packages/packages.go)、[WriteOverlays 实现](https://raw.githubusercontent.com/golang/tools/v0.50.0/internal/gocommand/invoke.go)。

## 8. 构建配置对固定快照的影响

| 维度 | 官方事实 | Narrant 建议 |
| --- | --- | --- |
| `go list` / `-e` / `-export` | `-e` 可返回带错误的包；export 模式可能需要构建；不是“只读解析一个 AST”的同义词。[go list](https://pkg.go.dev/cmd/go#hdr-List_packages_or_modules)、[driver 源码](https://raw.githubusercontent.com/golang/tools/v0.50.0/go/packages/golist.go) | 检查顶层 error、每包 Errors/TypeErrors、选中文件覆盖和类型证据；不把 exit 0/非空 package slice 当完整成功 |
| `go.mod` / `go.work` | workspace 的 use 明确列模块，replace 可覆盖模块配置；GOWORK 自动搜索父级；workspace 可维护 go.work.sum。[workspaces](https://go.dev/ref/mod#workspaces) | 显式设 `GOWORK=off` 或该侧仓库内确定 go.work 路径；每个内部 module 独立分组；不读父级用户 workspace；拒绝越界 use/replace，不重写配置副本 |
| 多 module、replace | 不同模块可有不同最低 Go 版本和同名 package；本地 replace 不由 go.sum 固定内容。[replace](https://go.dev/ref/mod#go-mod-file-replace) | 冻结包含两侧所有选中项目的兼容 SDK/profile；模块版本、目录和替换来源进入 cache key；跨仓库 replace 默认不可用，缺失依赖按证据降级 |
| vendor | vendor 模式不使用 module cache 下载，需与 modules.txt 一致；Go 1.22 支持 workspace vendor。[vendoring](https://go.dev/ref/mod#vendoring)、[Go 1.22](https://go.dev/doc/go1.22#go-command) | 保留原仓库 vendor 模式；不要为了方便强设 readonly 导致绕过 vendor；单 module vendor 与 workspace vendor 分别测试；不运行 `go mod vendor` |
| `GOOS` / `GOARCH` / tags | 平台、文件名后缀、release tags、架构特性和自定义 tags 决定文件集合。[build constraints](https://pkg.go.dev/cmd/go#hdr-Build_constraints) | 两侧使用同一显式 profile，记录 GOAMD64/GOARM 等相关特性；不能从本机平台推断仓库所有平台。已选但 inactive 文件仍需语法提取，语义受限明确表示；不能静默消失 |
| gopls 自动 build | gopls 会按打开文件推断部分 build；查询一般在该文件 default build 执行，不能覆盖所有 tags。[gopls workspace builds](https://go.dev/gopls/workspace#workspace-builds) | 若采用 gopls，禁止将自动推断当“所有平台完整图”；不同 build 的同名条件声明不可互相混合类型信息 |
| cgo | `import "C"` 隐含 cgo 约束；启用 cgo 需可用 C 编译器，交叉编译还有目标工具链要求。[cgo](https://pkg.go.dev/cmd/cgo) | 首版建议 `CGO_ENABLED=0`，仍提取选中 cgo 源码声明但 C 相关语义未知；需明确 profile 的降级。不能 FakeImportC 后把空 C package 视为真实解析；后续启用须有独立受控 C 工具链方案 |
| tests | Tests=true 可产生普通包、增强测试包、外部测试包、synthetic test main。[packages.Tests](https://pkg.go.dev/golang.org/x/tools/go/packages#Config) | 默认排除 `_test.go`；开启只恢复选中文件，不把 synthetic test binary/main 加图；生产声明跨变体去重，测试引用 production 时回到同一 ID。普通文件中名字像 TestX 的函数不直接视作测试 |
| generated / embed | go generate 必须显式运行；它可启动任意命令。embed 所需文件缺失可能影响包加载。[go generate](https://pkg.go.dev/cmd/go#hdr-Generate_Go_files_by_processing_source)、[embed](https://pkg.go.dev/embed) | 不运行生成器；读取已固定内容。生成源码完整时正常提取；缺少生成声明/嵌入资源报告准确原因；不把 `//go:embed` 文本变成已经找到文件的证据 |

**inactive selected files 是必须做的产品决定。** 共同输入要求所有已选、支持的对象被提取，而 Go semantic build 只包含某种 profile。建议第一版“每个已选文件都做语法抽取，只有所属有效 build 的符号建立确定关系；被排除文件的引用保留 Unresolved”。另一可行方案是对有限显式 profile 分别生成图，但需要定义 profile 身份和同声明去重，不能混合两个互斥文件后直接 type-check。现有 UI 没有丰富 coverage 字段；若无法清楚传达“对象完整、语义受限”，应为该 profile 明确失败，而非偷偷少产图。

## 9. SDK、标准库、依赖与离线策略

**事实：至少区分三层版本。** ① helper/gopls 的构建 Go 和集成 runtime，②其执行 `go list` 的实际 `go` 工具链，③项目的语言版本/构建约束。gopls 官方明确区分这三者，并把运行 go command 的支持范围限定为最新两个主要版本；源码语言版本可更旧。这项限制属于 gopls，不能不经验证套到自建 helper。[gopls supported Go versions](https://go.dev/gopls/#supported-go-versions)。

**事实：** 项目 `go` 指令是最低版本及语言语义输入，并不锁定唯一 SDK；`toolchain` 是主模块的建议。默认 `GOTOOLCHAIN=auto` 可下载新工具链；`local` 不自动切换；`<name>+path` 可寻址已有工具链但不下载。仅设 `GOPROXY=off` 也不能取代明确工具链策略。[Go toolchain selection](https://go.dev/doc/toolchain#select)、[go 指令](https://go.dev/doc/modules/gomod-ref#go)、[toolchain 指令](https://go.dev/doc/modules/gomod-ref#toolchain)。

**建议支持包只分发 helper，不默认再包一套 SDK。** helper 内含 parser/type checker，足以启动和做语法抽取；语义 readiness 仍需要已经安装且兼容两侧项目的 SDK。把某个 SDK 另附为固定“分析 SDK”可以研究，但它不能冒充项目历史工具链，必须另定 ADR、包容量/权限/安全更新策略和不兼容项目行为。标准库语义应来自实际选定项目 SDK 的 GOROOT，而不是 helper 构建时偶然使用的 SDK；两个来源混搭 export data 与类型检查器的兼容性也要测试。

**未验证的兼容范围必须显式关卡化。** helper 构建时绑定的 `go/parser`/`go/types` 版本与 driver SDK 并非无限可互换；更高版本语法、export data 格式、标准库 API 和文件语言版本都可能超出 helper 能力。对每个发布组合列支持矩阵，超出范围硬失败；禁止把不同 checker/load 的 types.Object 或 Universe 对象直接混用做 `types.Implements`。本机 Go 1.23.8 探针不能证明 Go 1.27.1 + x/tools v0.50.0 的兼容性。[packages 的类型身份约束](https://raw.githubusercontent.com/golang/tools/v0.50.0/go/packages/packages.go)。

**建议分析快照记录：** helper 版本、协议版本、x/tools 版本、实际 go 路径/版本、GOROOT 指纹、每 module `go` 语义版本、workspace/module/replace/vendor 配置摘要、GOOS/GOARCH/架构特性、tags、cgo、include_tests、依赖可用性与来源。先选择能覆盖 before/after 的共同兼容 SDK，再冻结 profile；如果用户要精确复现两侧各自历史 SDK，需要另一个比较模式，不能在一次比较中隐式漂移。

默认环境建议是 **待验证的受控策略**，不能仅凭列出变量就声称已完全隔离：

```text
GOTOOLCHAIN=local
GOENV=off
GOPACKAGESDRIVER=off
GOWORK=off 或该侧固定仓库内的 go.work 绝对路径
GOPROXY=off
GOSUMDB=off
GONOPROXY=none
GOTELEMETRY=off
CGO_ENABLED=0（首版建议）
GOOS/GOARCH/架构特性 = 已冻结的 profile
GOPATH/GOMODCACHE = 应用隔离目录，不接入用户第三方缓存，也不预填业务依赖
GOCACHE/GOTMPDIR = 应用拥有的源码仓库外缓存/临时目录
GOFLAGS = 经验证的 -mod=readonly 或原本有效的 -mod=vendor 等白名单
```

`GONOPROXY=none` 是为避免继承的 private/no-proxy 规则让依赖走 direct；还需清理继承的 GOFLAGS、GOEXPERIMENT、GOWORK、driver、tool wrapper/overlay、GONOSUMDB、GOPRIVATE 等影响。完整环境应从允许列表构造，当前路线不提供联网准备第三方依赖模式。`GOSUMDB=off` 不等于验证已有模块完整性，也不能用“离线”替代来源边界校验。

**实测发布阻断：`-mod=readonly` 不是“目录不可写”保证。** Go 1.23.8 中，新建临时 workspace `use ./app`，app require 一个已通过本地 file:// proxy 预填临时 module cache 的依赖，原先没有 go.sum/go.work.sum。随后断网策略下执行 `go list -mod=readonly -e -deps -json .` 返回 exit 0，**新增含两行摘要的 go.work.sum**；源码、go.mod/go.work 没变。实验没有联网，也未复制被审查项目源码。此预填缓存是隔离 fixture 的历史反例，不授权生产读取第三方 module cache；其证明的 readonly 限制仍需保留。当前 Go modload 源码的 workspace 分支仍明确调用 WriteGoSum。[modload/init.go](https://go.dev/src/cmd/go/internal/modload/init.go)、[module build commands](https://go.dev/ref/mod#build-commands)。

ADR-0031 的最终 tracked 内容检查抓不到这类新增 untracked 文件，更不能作为防写手段。Go 首版 release gate 必须实现无复制的原位只读执行/受控 metadata loader，或在不能保证只读的上下文明确失败；只读文件系统方案在 macOS/Windows 的实际适用性尚未验证。不能把 `go.work.sum` 写后回滚称为遵守契约；不得复制 go.mod/go.sum/go.work 到临时目录，也不能用 `-modfile`/磁盘 overlay 绕过现行不复制配置/源码规则。gopls 同样会使用 go 命令，换成 LSP 不会自动消除此 blocker。

### 离线缺少依赖时保留哪些事实

| 状态 | 可以保留 | 必须降级/失败 |
| --- | --- | --- |
| 第三方 import 不存在，但选中 `.go` 可读且语法完整 | 全部已建模声明、自身内容、包含/声明所有权；不经过未知类型的可靠同包具名引用/调用；独立标准库目标 | 缺失包、由其返回/嵌入/alias/约束传播的 selector 和接口关系 → Unresolved；不造空 import stub 的“已解析”目标 |
| 仓库已提交 vendor 或仓库内 replace 源码，符合项目规则且元数据完整 | 经真实导入和类型证据确认的仓库目标；仍遵守 selected Node 集合 | 不能因第三方来源一概删除仓库源码，也不能绕过来源、测试或项目规则 |
| 本机已有第三方 module cache 或仓库外业务依赖 | 不作为默认上下文加载 | 缺少允许范围内证据的相关引用保持 Unresolved，不能因缓存命中而扩大范围 |
| SDK 不存在、版本超出 helper 能力、go list 不能工作 | 可展示已有 File Changes；以后可提供显式语法模式 | 当前完整 Go 结构比较应失败并说明缺项；不能回退空图或无提示地宣称语义完成 |
| 选中文件损坏/不可读、helper 崩溃、超时/取消、非法路径 | 无成功图 | 硬错误传播，停止并回收 sibling；不是“可忽略诊断” |
| 仅 package clause 的合法无声明文件，或被 include_tests 明确排除 | 前者按本建议保留 file header，后者无节点 | 必须先校验路径与读取；完全空的 `.go` 属语法错误，不因公共输入允许选它就成功空图 |

**建议实现证据门槛：** 逐条引用确认 `types.Object` 的来源/种类、有效类型和解析路径；依赖错误污染的关系保留 Unresolved。`Package.IllTyped` 不是必须清空全包的理由，但部分 Info 中出现对象也不是所有相邻推断都可靠的保证。建立缺失依赖、损坏导入、shadowing、带错误泛型实例的负例，优先宁缺毋滥。报告层面区分“工具失败”与“上下文缺失”；公共 CodeGraph 当前没有诊断封套，若用户需看到覆盖率应另外设计 application 状态，不把诊断塞进参与比较的 extensions。[types.Info 错误行为](https://raw.githubusercontent.com/golang/go/go1.27.1/src/go/types/api.go)。

## 10. 固定支持包、平台与再分发

建议 Go 包包含：

```text
bin/narrant-go-analyzer[.exe]
licenses/Go-LICENSE
licenses/x-tools-LICENSE
licenses/<实际链接第三方组件的许可证>
control/build-info.json   # 固定源码提交、Go/x-tools 版本、协议及能力版本
```

**建议：** helper 是 Narrant 自有受控入口，CI 用固定 Go/x/tools、锁定模块摘要构建；运行时不 `go run` 源码、不 `go install @latest`。纯 Go、禁用 cgo 的 helper 适合为 darwin/arm64 与 windows/amd64 构建，但“可以交叉编译”不等于已经在目标平台运行验收。支持包仅有上述两平台是现仓库目标，不应在文档承诺 Linux/Windows ARM64 已支持。

**再分发事实：** Go 和 x/tools 顶层 BSD 3-Clause 条款允许源/二进制再分发，需保留版权、条款、免责，不得未经许可用贡献者名义背书；必须随实际二进制依赖核验第三方许可证，不能用顶层 LICENSE 覆盖全部依赖。[Go LICENSE](https://go.dev/LICENSE)、[x/tools 固定标签 LICENSE](https://raw.githubusercontent.com/golang/tools/v0.50.0/LICENSE)、[gopls 固定标签依赖](https://raw.githubusercontent.com/golang/tools/gopls/v0.23.0/gopls/go.mod)。Go 官方归档提供平台文件与 SHA256；若未来分发 SDK，须锁定原始归档/摘要和其中附加 notices，不只复制 `bin/go`。[官方下载页](https://go.dev/dl/)。

**现有打包陷阱：** [`packageDirectory`](../../scripts/build-language-support-packs.mjs) 仅把 `bin/*` 标 executable；若未来整个 SDK 放在 `runtime/go`，其 `pkg/tool/<host>/compile` 等程序不会自动恢复执行权限。当前归档上限 256 MiB、解压后上限 1 GiB，新增 SDK 需要实测容量；manifest 不允许 symlink/hardlink/special file。最小 helper 包可绕过 SDK 体积，但不能因此省略项目 SDK readiness。[storage](../../src-tauri/src/language_support/storage.rs)。

## 11. 取消、子进程、性能与缓存

**事实：** packages.Config.Context 可让 Load 在取消时中止，但它不是 Narrant 的完整进程树回收证明；类型检查阶段也没有统一、逐 AST 节点的强取消合同。[Config.Context](https://pkg.go.dev/golang.org/x/tools/go/packages#Config)。

**建议：** 每侧、每次分析一个 helper worker（内部可分项目），Rust 传 request id、绝对截止时间与取消信号；helper 把 context 传给 packages，遍历节点时主动检查。超时/取消后先请求退出，短宽限后杀进程组/Windows Job 并 wait；同时 drain 限额 stderr/stdout，防止阻塞和无限输出。go/compiler/cgo 等孙进程必须被纳入所有权，不能只 kill helper。支持包租约和 Review Workspace lease 在清理完成后才释放。

**当前复用限制：** [`lsp/transport.rs`](../../src-tauri/src/lsp/transport.rs) 的 `ServerProcess` 是私有 LSP 实现，已有 Unix process group 与 Windows Job（suspended spawn）策略。不能直接声称非 LSP helper 已可无改动复用；建议抽出受控进程所有权层，或提供等价实现并复用同类生命周期测试。原研究的 gopls 候选可更直接使用 LSP transport，但其 go 子进程仍需回收测试。

**性能建议：** 先限制 root package 数量并去重，metadata 与 type-check 分阶段计时；不要为每个调用发一条 LSP 请求，也不要默认全仓扫描所有实现候选；隐式实现按已加载接口/类型候选索引，避免所有类型两两比较。before/after 各一份内存类型域，限制 package 并发和 CPU，避免语言并行再叠加无限 go 编译。Go cache 是编译缓存，CodeGraph cache 是 Snapshot 结果，两者不可混淆。

**缓存建议：** key 包括 Snapshot、side、选集、include_tests、helper/协议/x/tools、SDK/标准库、module/work/work.sum/vendor manifests、tags/平台/cgo、允许上下文的解析结果。项目内容变化不能命中另一侧的 graph。Go GOCACHE 可按工具链和 profile 管理，但不得借缓存复用带入用户第三方模块；默认输入边界不因缓存命中改变。gopls 已有磁盘索引与内存 cache，但不会自动拥有 Narrant 的 Snapshot 身份。[gopls scalability 设计注记](https://go.dev/gopls/design/design)。

## 12. 具体接入位置

本节仅定位后续工作，**本次没有实施**。

| 位置 | 后续变更 |
| --- | --- |
| `src-tauri/src/structure_review/go_analyzer/`（新增） | 实现 LanguageAnalyzer、输入来源边界、协议、通用校验、最终图构造与生命周期；独立 `tests/` 验证路径、选集完整性、错误/取消 |
| Go helper 源码目录（建议 `tools/go-analyzer/`） | 负责 project / syntax-content / identity / semantic / graph，确定图成员及关系分类，返回规范化 CodeGraph 构造 DTO；固定 go.mod/go.sum，中文可读函数/结构注释。协议字段与编码仍待设计，生产二进制进支持包，分析时不编译 helper |
| [`structure_review/mod.rs`](../../src-tauri/src/structure_review/mod.rs) | 注册 Go 模块 |
| [`application/mod.rs`](../../src-tauri/src/structure_review/application/mod.rs) | 扩展 AnalyzeGo、LanguageAnalyzers、prepare_analyzers、SideFiles、LanguageGroup/language_group、analyze_side/merge；`.go` 识别；两个 side 使用同一配置快照 |
| [`language_support/model.rs`](../../src-tauri/src/language_support/model.rs)、[`storage.rs`](../../src-tauri/src/language_support/storage.rs) | PackId::Go、slug、required entries、枚举扫描；构建信息/许可证必需项；保持校验/租约 |
| [`resources/language-support-packs.json`](../../resources/language-support-packs.json) | 独立包版本、固定 helper commit、Go/x-tools build 版本、每平台 archive SHA256 |
| [`scripts/build-language-support-packs.mjs`](../../scripts/build-language-support-packs.mjs) | validateIdentity、默认 IDs、目标映射、构建/打包、native smoke；若附 SDK，修正可执行文件规则 |
| [`tauri_app/bootstrap.rs`](../../src-tauri/src/tauri_app/bootstrap.rs)、[包构建工作流](../../.github/workflows/language-support-packs.yml) | 开发/生产装配保持一致；加入两平台 helper 原生语义查询、退出和安装验证，不以 `--version` 代替验收 |
| [`lsp/transport.rs`](../../src-tauri/src/lsp/transport.rs) 或新通用受控进程层 | 可回收 Go helper 及 go/compiler 子进程；不能只借用 CancellationCheck 而缺进程所有权 |
| [前端包类型](../../src/language-support/types.ts)、[结构分析错误白名单](../../src/structure-review/use-structure-review.ts)、settings / transport / i18n | Go 包名、配置/readiness 和错误展示；新增类型通过现有 transport 流程；Go 源码编辑器映射需核对；若调整交互，需 pnpm 检查和应用内浏览器验收 |

Rust tests 仍放模块内 tests/，所有 Cargo 通过 `scripts/cargo-agent.sh`；不手动触发 quality-gate。语言包 native 验证与项目工具链验收是不同检查。

## 13. 可执行最小实验与验收矩阵

下面实验只依赖已安装 Go 标准库，不下载模块，验证“AST/类型证据与函数值不同”这一最小前提；不代表完整 packages helper、跨平台、LSP、固定支持包或性能已经通过。它只在新建临时目录写自己生成的 fixture，不复制仓库语言源码，也不改项目配置。

```sh
probe_dir=$(mktemp -d "${TMPDIR:-/tmp}/narrant-go-probe.XXXXXX")
cat > "$probe_dir/main.go" <<'GO'
package main

import (
    "fmt"
    "go/ast"
    "go/parser"
    "go/token"
    "go/types"
)

// main 仅检查内存里的 Go 样例，输出类型证据而不执行样例程序。
func main() {
    const src = `package p
type I interface { M() }
type T struct{}
func (*T) M() {}
func Target() {}
func Id[A any](a A) A { return a }
func Use() { Target(); f := Target; f(); _ = Id[int](1) }
`
    fs := token.NewFileSet()
    f, err := parser.ParseFile(fs, "probe.go", src, parser.ParseComments|parser.SkipObjectResolution)
    if err != nil { panic(err) }
    info := &types.Info{
        Defs: make(map[*ast.Ident]types.Object),
        Uses: make(map[*ast.Ident]types.Object),
        Instances: make(map[*ast.Ident]types.Instance),
    }
    cfg := types.Config{GoVersion: "go1.23"}
    pkg, err := cfg.Check("example.test/p", fs, []*ast.File{f}, info)
    if err != nil { panic(err) }
    t := pkg.Scope().Lookup("T").Type()
    i := pkg.Scope().Lookup("I").Type().Underlying().(*types.Interface)
    fmt.Printf("implements T=%v pointer=%v\n", types.Implements(t, i), types.Implements(types.NewPointer(t), i))
    ast.Inspect(f, func(n ast.Node) bool {
        call, ok := n.(*ast.CallExpr)
        if !ok { return true }
        if id, ok := call.Fun.(*ast.Ident); ok {
            fmt.Printf("call %s object=%T\n", id.Name, info.Uses[id])
        }
        return true
    })
    for id, inst := range info.Instances {
        fmt.Printf("generic %s arguments=%d\n", id.Name, inst.TypeArgs.Len())
    }
}
GO
env GOTOOLCHAIN=local GOENV=off GOWORK=off GOPROXY=off GOSUMDB=off \
    GONOPROXY=none CGO_ENABLED=0 \
    GOCACHE="$probe_dir/cache" GOTMPDIR="$probe_dir" \
    go run "$probe_dir/main.go"
```

预期：`T=false pointer=true`；`Target` 调用的 object 是 `*types.Func`，`f` 调用是 `*types.Var`；Id 有一个类型实参。此预期是可检验断言，不应仅因文字输出相似就算完整 Go analyzer 验收。

### 原位只读的最小反例

以下 Python 3 脚本调用已有 Go，不访问网络。它在临时目录创建本地模块代理，以 `file://` 预填专用缓存，再设置 `GOPROXY=off` 进行 workspace 分析。Go 1.23.8 的实际结果为 `exit=0`、`created_go_work_sum=true`；这不是希望产品满足的行为，而是接入前必须防住的反例。`TemporaryDirectory` 仅回收实验文件，不代表生产可以修改工作区后回滚。

```python
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile

go = os.environ.get("NARRANT_GO_PROBE_BIN") or shutil.which("go")
if not go:
    raise SystemExit("需要已有 Go SDK；本实验不安装工具链")

with tempfile.TemporaryDirectory(prefix="narrant-go-worksum-") as directory:
    root = Path(directory).resolve()
    proxy = root / "proxy/example.test/dep/@v"
    proxy.mkdir(parents=True)
    mod = "module example.test/dep\n\ngo 1.23.0\n"
    (proxy / "v1.0.0.mod").write_text(mod)
    (proxy / "v1.0.0.info").write_text(
        '{"Version":"v1.0.0","Time":"2024-01-01T00:00:00Z"}'
    )
    (proxy / "list").write_text("v1.0.0\n")
    with zipfile.ZipFile(proxy / "v1.0.0.zip", "w") as archive:
        archive.writestr("example.test/dep@v1.0.0/go.mod", mod)
        archive.writestr("example.test/dep@v1.0.0/dep.go", "package dep\nfunc F(){}\n")

    workspace = root / "workspace"
    app = workspace / "app"
    app.mkdir(parents=True)
    (workspace / "go.work").write_text("go 1.23.0\nuse ./app\n")
    (app / "go.mod").write_text(
        "module example.test/app\n\ngo 1.23.0\nrequire example.test/dep v1.0.0\n"
    )
    (app / "app.go").write_text(
        'package app\nimport "example.test/dep"\nfunc Run(){dep.F()}\n'
    )
    env = {
        **os.environ,
        "GOTOOLCHAIN": "local", "GOENV": "off", "GO111MODULE": "on",
        "GOWORK": "off", "GOFLAGS": "", "GOEXPERIMENT": "",
        "GOPACKAGESDRIVER": "off", "GOTELEMETRY": "off", "CGO_ENABLED": "0",
        "GOPROXY": (root / "proxy").as_uri(), "GOSUMDB": "off",
        "GOPRIVATE": "", "GONOPROXY": "none", "GONOSUMDB": "",
        "GOCACHE": str(root / "cache"), "GOPATH": str(root / "gopath"),
        "GOMODCACHE": str(root / "modcache"), "GOTMPDIR": str(root),
    }
    subprocess.run(
        [go, "mod", "download", "-json", "example.test/dep@v1.0.0"],
        cwd=root, env=env, check=True, capture_output=True, timeout=30,
    )
    env.update(GOPROXY="off", GOWORK=str(workspace / "go.work"))
    result = subprocess.run(
        [go, "list", "-mod=readonly", "-e", "-deps", "-json", "."],
        cwd=app, env=env, text=True, capture_output=True, timeout=30,
    )
    print(json.dumps({
        "exit": result.returncode,
        "created_go_work_sum": (workspace / "go.work.sum").exists(),
        "stderr": result.stderr,
    }, indent=2))
```

### 验收矩阵（后续实现必须执行）

| 场景 | 必须成立的断言 | 级别 |
| --- | --- | --- |
| 同一 fixture 放在两个绝对目录 | Node/External ID、signature/code、references 完全相同 | helper golden + integration |
| 只改注释/排版；只改函数体；只改子方法 | 前者无 diff；body 有 diff；父自身内容不重复子改动 | graph diff |
| 同名包/别名 import/内部同名 shadowing | 无裸名碰撞、错误绑定或不确定 External | semantic |
| receiver T/*T、promoted method、私有跨包方法名 | 只产生真实方法集关系；promotion 回到原声明 | semantic |
| generic 函数/类型/alias、约束、错误实例 | 身份归一、签名保留；不把可能实例化当确定 implements | semantic negative |
| 局部 type、闭包、方法值、函数值、接口派发、反射 | 自身内容不丢；动态调用保留 Unresolved；不猜唯一实现 | semantic negative |
| 多 init、iota、共享多返回初始化 | 身份无冲突；顺序/派生值变化可见；不伪造重复初始化 | content |
| selected / unselected 同包、其它仓库包、stdlib、缺依赖 | Node/Repository External/Dependency External/Unresolved 四类准确 | boundary |
| go.work/use/replace 越界与 symlink | 加载前拒绝；没有外部读取成功伪装固定快照 | isolation |
| module / workspace vendor、一致/不一致 modules.txt | 正确选择模式；不静默更新/下载 | project |
| tags、GOOS/GOARCH、架构特性、非激活 selected 文件 | 两侧 profile 一致；互斥文件不混合；selected 内容不静默消失 | project |
| cgo disabled / enabled 缺 CC、`//line`、生成代码 | 无伪 C 绑定、无临时路径身份、无生成器执行 | boundary |
| include_tests 开关与 package p / p_test | 默认排除明确测试；开启限选集；production 变体去重 | extraction |
| 包加载 top-level error / Package.Errors / helper crash | 硬失败与上下文缺失区分；不空图成功 | lifecycle |
| Go SDK 缺失/过旧、GOTOOLCHAIN auto 污染、GOPRIVATE/GONOPROXY | 不下载工具链/模块；诊断可行动 | environment |
| readonly 下 go.sum/work.sum 不完整 | 工作区前后字节不变，包括新生成未跟踪文件；若需写则明确失败 | release gate |
| 取消加载/类型检查、stdout 堵塞、peer fail、关闭/刷新 Review | Unix/Windows 所有孙进程结束，workspace lease 与 pack lease 才释放 | lifecycle |
| 两侧 + 多语言并行、大仓库冷/热缓存 | 记录总时长、load/type/check/emit、RSS、子进程峰值；无隐含性能通过承诺 | benchmark |
| darwin arm64 / Windows x64 支持包安装与损坏包 | 摘要/权限/路径/许可证完整；真实 helper 启动成功 | native packaging |

### 本次已执行的轻量探针

本次实测均使用新建临时目录的自写 fixture 和本机 Go 1.23.8；没有网络下载、安装 SDK、运行被审查代码、Cargo 或大型项目编译。方法集探针编译运行的是自写分析 helper，样例本身仅被解析和类型检查。项目加载探针固定 local toolchain、GOENV off、GOPROXY/GOSUMDB off、GOTELEMETRY off、GOPACKAGESDRIVER off、CGO disabled、Darwin/arm64，以及临时 GOPATH/GOMODCACHE/GOCACHE；workspace sum 反例预填缓存时只读取自建 file:// proxy。以下不是发布候选版本的全矩阵验收。

| 实际探针 | 实际观察 |
| --- | --- |
| AST + types，T 仅声明 `(*T).M` | `Implements(T,I)=false`、`Implements(*T,I)=true`；`t.M()` 可绑定指针方法；`i.M()` 只绑定 I.M 接口槽位 |
| `Id[int]`、参数函数值 `cb()`、`f:=Local; f()` | Id 的 Uses 为原 Func 且 Instances 有实参；后两种 callee 仅为 Var，不能直接推断 Local 为调用目标 |
| 缺失 import 的 importer 返回明确 error | 可靠本地 `Local()` 仍有 Func；缺失包调用无确定 selector 证据；这证明可保留部分可信信息，不证明所有错误程序可安全提全关系 |
| 两个 `init` 声明 | parse/type-check 合法，身份需要特别消歧 |
| go list 构建/test/generated | Darwin build 纳入 a.go/generated.go，Windows 文件在 IgnoredGoFiles；TestGoFiles/XTestGoFiles 区分；`-test` 返回原包、测试增强包、外部测试包和位于 cache 的 synthetic main |
| 缺失 import + `go list -e` | exit 0，同时有 Incomplete/DepsErrors，退出码不能判断语义成功 |
| module `go 1.99` + GOTOOLCHAIN=local | exit 1，明确版本不符，无工具链下载 |
| go.work + 仓库外本地 replace | `-mod=readonly -e -deps` 成功返回仓内 lib 与越界 ext 的 Dir；GOWORK/readonly 都不限制仓库边界 |
| 独立 module vendor | 离线 `-mod=vendor` 从仓内 vendor 解析依赖，保留 Module.Path/Version；物理来源仍在仓库内 |
| 已有完整 metadata 的普通 fixture | 此组源码/config hash 前后不变；不能推广所有 go.work |
| workspace sum 反例 | 离线 `-mod=readonly` 仍新建 go.work.sum；这是已观察到的 blocker，不是仅理论风险 |

**文档脚本复核：** 本节两个代码块已从最终文档提取并实际执行。标准库 helper 输出 `implements T=false pointer=true`、`call Target object=*types.Func`、`call f object=*types.Var`、`generic Id arguments=1`；workspace 反例输出 `exit: 0`、`created_go_work_sum: true`、空 stderr。两者均使用本机 Go 1.23.8。

**未执行：** 候选 x/tools v0.50.0 `packages.Load` helper、gopls LSP 会话、Go 1.27 语法/标准库矩阵、cgo、Windows native、真正只读沙箱、大仓库性能、Narrant 端到端与支持包安装。本次证据足以选择后端方向和揭示加载风险，不能证明完整 Go 语言支持已实现或可发布。

## 14. 实施阶段与待用户决定的问题

1. **契约与最小语义原型。** 固定候选 helper/Go/x-tools 版本；完成函数、方法、type、field、var/const、import 和保守 calls；证明本地/标准库/缺依赖边界、稳定身份、selected coverage、只读加载。不先扩大到 SSA、全仓实现搜索或 cgo。
2. **Narrant 接入。** 受控进程层、LanguageAnalyzer、两侧 application、固定包、SDK readiness、取消/回收；将成功结果交给 CodeGraph 统一验证与 diff。所有硬错误贯穿完整结构比较。
3. **复杂项目。** go.work、多 module、vendor、tags/profile、tests、生成代码、Go 版本矩阵；定义实测支持边界，并用真实目标平台安装包做验收。
4. **按需要扩大。** 独立闭包、初始化顺序对象、精确接口实现展示、函数值 SSA、cgo、多 build profile；每项先补语义与性能证据。外部业务依赖来源不属于当前默认后续路线。

除 ADR-0034 已确认的架构、执行与默认输入范围外，实施前仍需决定的产品问题：

- 项目 SDK：首版沿用 Rust 的“已有兼容 SDK，缺失明确失败”，还是提供独立固定分析 SDK？允许哪些旧 Go 项目，是否要求两侧共同 SDK？
- 条件构建：默认 profile 是宿主平台还是仓库指定平台？inactive selected 文件的“结构完整、语义受限”如何呈现？是否需要多个 profile 图？
- cgo：首版禁用并明确语义缺口，还是必须支持受控本机 C 工具链？无论如何，不执行仓库及依赖携带的脚本/插件/生成器；仓库外业务 replace/use 不属于当前默认输入。
- 关系：`*T` 的接口实现如何显示？接口调用是 Unresolved + contract reference，还是允许 `code.calls` 表示静态接口槽位？是否需要所有隐式实现，还是只报告已加载范围中有证据的关联？
- 对象粒度：闭包和局部 var/const 是否独立对象；包初始化、iota、多名共享 RHS 采用哪种独立展示粒度？其顺序与唯一执行归属必须正确比较，不是可选的后期增强。

**不需要另行改变的底线：** 固定两侧工作区、不复制源码/配置、不安装项目工具链、不伪造确定关系、硬错误不变空图、选集外只作上下文、取消后完成子进程清理。这些来自当前契约，不是本调研新增的审批流程。

## 15. 后续源码 importer 探针（2026-10-02）

**实测新增证据：** 已在 Go 1.23.8 / darwin arm64 上，使用 `go/parser` + `go/types` + 严格源码 importer，分别按 linux/amd64 和 windows/amd64 解析独立临时样例。公共 `Run()` 对两个平台实现的调用分别绑定到对应源文件，合并观察后保留两种目标。仓库另一包与 `cmp.Compare`、`strings.Contains` 均能解析；第三方包即使已放在专用 GOPATH 中仍被拒绝，该调用保持未知，可靠的本地和标准库调用证据仍可取得。

样例分析阶段没有 `go list` / `go/packages` 项目导入或标准库导出数据生成；直接递归解析指定 GOROOT 的标准库声明即可完成本例类型绑定。分析前后 fixture 全部路径与文件字节一致，没有生成 `go.work.sum`。这仅证明本次样例没有持久化修改，不是通用只读保证。复跑脚本、明确断言、观察结果及限制见 [源码加载探针](probes/go-native-helper/README.md)。

**对原建议的修正：** 默认 `go/packages` driver 仍有此前已证实的原位写入 blocker，不能视为可直接采用的确定加载方案；新证据表明“受限静态项目模型 + 源码 importer + `go/types`”值得优先做下一轮 loader 验证。也可评估基于同一可信元数据的自定义 packages driver，但不得回退默认项目导入。探针手工指定仓库包与两个配置，没有解释 go.mod/go.work，没有构建完整 CodeGraph，也未证明自动配置发现、对象完整性、身份/包含归并、全部标准库、cgo 或跨 SDK 兼容。不能因此声称完整 Go loader 或发布支持已完成。

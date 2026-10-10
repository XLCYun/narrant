# TypeScript / JavaScript Code Graph 接入清单

日期：2026-09-14。本文只读核查当前 Rust 实现、公共图和应用接点，整理后续实现步骤与行为验收；没有实现 TS 适配器、编译、创建 Issue 或运行下列验收。

产品范围采用 [设计草案的确认记录](typescript-code-graph-plan.md#本轮已确认的范围)，Q5–Q8 不再作为待选项。后端探针及其实际覆盖范围由 [LSP 验证报告](typescript-lsp-validation.md) 记录；本文的“待验证”指生产集成验收，不能把已通过的小样本扩展成通用实现已完成。术语沿用 [GLOSSARY.md](../../GLOSSARY.md)。

## 已接受的边界

- TS/TSX、JS/JSX、MTS/CTS/MJS/CJS 及声明文件由同一语言适配器覆盖；TS 与 JS 对象仍保留各自语言标识。标准源码以外的框架文件不因包含 TS 文本而自动进入支持范围。
- 默认语义上下文为本侧仓库代码、ECMAScript 基础库及项目所需的受控 DOM/Node 运行时声明。第三方业务源码和声明不进入默认语义上下文。工具不自动安装项目依赖、运行项目构建或配置脚本。
- 主方案为 Oxc 加 LSP。工程推荐 TLS 6.0.0 + tsserver 6.0.3，通用输入采用保留项目结构的分析目录；noResolve 平铺项目在 references 样例已失败。LSP 不等同于 TS7；不在 before/after 两侧静默切换后端。
- Q5：直接 `const` 普通命名绑定的箭头独立成为 Callable；链式调用中的匿名参数箭头内联保留。不新增框架包装器识别。
- Q6：显示已解析到图内仓库对象的组件使用关系；HTML intrinsic 标签没有独立对象或组件边，其源码仍参与 owner 的内容比较。
- Q7：跨文件 interface/namespace 合并声明各自保留来源和对象，首版不跨文件合卡。语义查询仍识别符号的全部相关声明。
- Q8：按 `*.test.*`、`*.spec.*`、`__tests__/` 等明确文件约定识别测试，复用默认关闭的 `include_tests`。不凭 `test/it/describe` 名字删除混合业务代码；未知或共享代码保留。
- JSDoc 原文和后端已有解析/推断能力自然保留，不增加专用解析器、契约模型或比较流程，也不主动过滤通用语义结果。
- 沿用当前开发工具路径约定：工具预先提供，缺失或不可用明确报错。自动下载和正式打包仍延期；不会把捆绑 Node 变成本轮新增产品问题。

这些边界与 [0018 公共契约](../specs/0018-structure-review-code-graph.md)、[0019 Rust 适配器](../specs/0019-rust-language-analyzer.md)、[0020 Structure Changes](../specs/0020-structure-changes-review-view.md) 一起约束实现。Rust 专用识别规则不机械复制到 TS；公共的单一 placement、own content 和失败边界继续适用。

## 当前代码事实与最小接入动作

| 接点 | 当前代码事实 | 必须完成的最小动作 |
| --- | --- | --- |
| 语言调用入口 | `AnalyzeGraph` 的参数绑定 `RustDependencyScope`，`StructureReview` 只保存 `analyze_rust`。见 [application/mod.rs](../../src-tauri/src/structure_review/application/mod.rs)，22–77 行 | 让应用在每侧把选中文件交给正确语言运行入口。Rust 保留原参数，TS 使用自己的固定服务配置；不要把 Rust 依赖枚举塞进 TS 的公共选项 |
| 文件覆盖 | `scope` 对 old/new 路径分别判断；`coverage_kind` 只允许普通 Git 文件和 `.rs`。见 [application/mod.rs](../../src-tauri/src/structure_review/application/mod.rs)，279–324 行 | 增加 TS 家族路由，保留 absent、unsupported-kind、unsupported-language 的区分；跨扩展名改名按两侧实际语言处理 |
| 分析结果 | `LanguageAnalyzer::analyze` 只返回图或错误；IPC 的 `StructureComparison` 只有两侧图、diff、coverage。见 [analyzer.rs](../../src-tauri/src/structure_review/code_graph/analyzer.rs)、[projection.rs](../../src-tauri/src/structure_review/application/projection.rs)，9–31 行 | 增加应用层每侧的运行说明承载，标明语言、项目/文件范围及缺配置/缺依赖影响。语言运行入口须能把说明交给应用；不能为保留旧返回值而丢弃信息 |
| IPC 类型 | 前端结果类型单独定义；Tauri command contract 引用它，适配器复用通用取消传输。见 [types.ts](../../src/structure-review/types.ts)，44–68 行与 [command-contract.ts](../../src/infrastructure/tauri/command-contract.ts)，37 行 | 应用报告新增字段时同步 Rust 序列化投影与前端结果类型，验证旧 Rust 结果可提供空说明；不另建一条 TS 专用前端 IPC 流程 |
| 两侧生命周期 | before 准备与 after 分析并行；实际 worker 都结束、before 清理完成后才返回。真实失败优先于伴随取消，清理失败保留根因。见 [application/mod.rs](../../src-tauri/src/structure_review/application/mod.rs)，95–225 行 | 保留两侧并行与同组取消。先在每侧顺序运行语言组，不增加语言数乘项目数的无界并行。任一语言硬失败时整次比较失败，不发布 Rust 成功、TS 缺失的伪完整图 |
| 选项冻结 | Tauri 命令在启动前读取一次 Rust 设置，两侧共用同一 `include_tests` 与 scope。见 [commands/structure_review.rs](../../src-tauri/src/tauri_app/commands/structure_review.rs)，25–50 行 | 同一次比较固定工具版本、标准库来源、映射规则、用户选项。before/after 仍读取各自真实 tsconfig/package.json；不能把“相同分析约定”实现成相同项目配置 |
| 工具接线 | bootstrap 只构造 Rust 分析器，环境路径与 `.tools` 默认路径不位于被分析仓库。见 [bootstrap.rs](../../src-tauri/src/tauri_app/bootstrap.rs)，108–128 行 | 增加固定 Node、TLS、tsserver 等工具的明确路径与可用性检查；不依赖用户当前 checkout 的 node_modules 或 PATH 恰好提供正确版本 |
| 设备设置 | `StructureReviewSettings` 当前只有 `rust` 字段。见 [application_settings/model.rs](../../src-tauri/src/application_settings/model.rs)，7–19 行 | TS 首版采用已确认默认配置，不为接入而强加新设置开关。将来若确需设备级配置，再按语言分开并冻结；兼容现有持久化文件 |
| 展示 | 图投影保留开放的 language/nativeKind/semantic；只显示图内 Node 引用。Rust `use` 的隐藏条件明确带 `language == rust`。见 [graph-presentation.ts](../../src/structure-review/graph-presentation.ts)，48–78 行 | 复用投影，不把 TS import/export 误命中 Rust use 规则。组件边使用独立语义，不能冒充 imports 或 calls；保留现有图外引用隐藏政策 |
| 文案与详情 | loading/unsupported 文案硬编码 Rust；源码详情已经使用节点语言和 source。见 [i18n.ts](../../src/lib/i18n.ts)，106–111、258–263 行；[structure-details.tsx](../../src/components/structure-review/structure-details.tsx)，45–62 行 | 更新语言覆盖文案，验证 TS/JS/JSX 原文显示。运行说明独立于对象差异；不恢复已移除的 coverage 明细 UI，也不显示服务器内部配置给普通用户作选择 |

快照执行继续遵循 [ADR 0031](../adr/0031-reuse-review-head-for-structure-analysis.md)：lease 串行化应用内工作区变更，检查前后固定 HEAD 与 tracked 内容；它不提供对外部进程、未跟踪文件的完整文件系统隔离。TS 默认范围要验证实际语言服务项目输入，不仅过滤 UI；也不因此新增任意文件读取沙箱要求。

## 共享 LSP 的复用与明确缺口

[LspClient](../../src-tauri/src/lsp/mod.rs) 已接受完整 `Command`，支持参数/环境、固定工作目录、initialize/initialized、明确 languageId 的 didOpen、任意标准/扩展 request、definitions 和两步 outgoing call hierarchy。默认协商 UTF-16。调用层级保留第二步所需 opaque item，返回 `from_ranges` 可定位源 occurrence。[协议数据](../../src-tauri/src/lsp/protocol.rs) 对 null、单项、数组和错误类型作区分；协议错误不能转换成无目标。

这些能力足以作为 TS 会话的传输基础，尚有以下必须验证或补齐的部分：

1. **项目初始化和就绪**：Rust 的 serverStatus/parentModule 是语言专用逻辑。TS 适配器应控制 external project 或分析视图、同步原文，再以真实服务响应确认项目输入与查询可用；不能只等 initialize 成功。TLS didOpen 之后的项目归属、多项目引用与配置发现均需验证。
2. **所需反向请求**：当前只确认 progress/create、register/unregisterCapability；其余返回 MethodNotFound。若选定 TLS 需要 workspace/configuration 或其他客户端能力，只增加实测必需的协议支持，或不宣告未实现能力。对固定快照不需要工作的动态 watcher 不能仅因注册成功就假称已有事件实现。
3. **诊断收集**：`latest_notification` 按 method 只保留最后一条；多个文件的 publishDiagnostics 会互相覆盖。不得把它当作全项目诊断存储。可选择受控项目/文件诊断查询，或补有界、按 URI/项目归属的收集机制。具体方式是工程选择，须验证不丢前一文件的关键说明。
4. **错误可读性**：[transport.rs](../../src-tauri/src/lsp/transport.rs)，56–60 行将 stderr 置空。优先使用明确的工具检查与 LSP/项目错误；若真实启动失败无法给出可操作信息，再增加有界 stderr 摘要。错误不能仅剩“没有结构变化”。
5. **位置映射**：Oxc span 是源码字节范围，LSP 是 UTF-16 行列；TS 适配器需要本侧原文的双向换算、边界验证与声明选择范围。`Location` 当前只保留目标起点；必须实测是否足以区分箭头绑定、overload、合并声明和 alias。若不足，保留额外证据，不能任取第一个位置。
6. **结束与取消**：共享 `shutdown` 当前发送 `params: null`。按选定 TLS 实测生命周期；原生 TS7 的参数兼容问题不自动外推到 TLS。Unix 进程组、Windows taskkill 的现有守卫可复用，但要验证 Node → TLS → tsserver 子进程都被回收。超时/初始化失败/请求失败也走相同清理路径。

以上通用层修改若有必要，需让现有 [LSP 会话测试](../../src-tauri/src/lsp/tests/session.rs) 与 [Rust LSP 测试](../../src-tauri/src/structure_review/rust_analyzer/tests/lsp.rs) 一起回归；不要把 TS 特定配置写入通用 LSP 默认值。

## 每侧图合并与运行说明

**语义分组先按适配器能力，而不是按输出语言标签。** Rust 一组；TS 与 JS 文件在同一适配器中共享受控项目上下文。若把 TS 和 JS 分别提图再简单拼接，本来应为图内 Node 的 TS→JS 引用可能错误地保留成 External。选择范围的最终成员表应在目标分类之前确定；不新增 TS→Rust 桥接推断。

公共 [CodeGraph](../../src-tauri/src/structure_review/code_graph/graph.rs) 当前没有 merge 方法，但 `root()`、`node()`、节点 Clone 和 `external_symbols()` 已足够遍历已验证图。最小组合可以保留在应用内部：收集各组根、完整节点和外部描述，再经 `CodeGraph::new` 校验，并附加外部描述。不必为了合并暴露可变全局节点表。

- 保留语言命名空间和本侧仓库相对身份。碰撞必须报错，不能让 BTreeMap 后写覆盖前写；不能按显示名合并 Rust/TS 对象。
- 合并后每个节点只有一个声明 placement，root 可达，包含关系无环，全部 Node 目标存在。跨语言引用只有适配器明确提供时才成立。[validation.rs](../../src-tauri/src/structure_review/code_graph/validation.rs) 已执行这些结构检查。
- 组合外部描述时不能静默覆盖相互矛盾的身份信息；同一已解析对象因是否被选中改变 Node/External 分类时，底层稳定身份保持一致。
- TS 同一源码被多个 tsconfig 包含时只输出一份对应声明；不同上下文给出冲突证据时需要可复核处理，不用项目索引或绝对路径给源码对象制造临时身份。这属于多项目验证项。
- 缺依赖且服务正常时保留声明、自身内容和可靠引用，观测到但无目标的引用保留 Unresolved。启动失败、超时、协议错误、所选文件读取/提取失败仍是错误；不能把语法图作为自动成功备选。
- 运行说明不进入 `CodeNodeData.extensions`、signature/code 或关系集合。[diff.rs](../../src-tauri/src/structure_review/code_graph/diff.rs)，73–95 行会比较这些语义数据；把项目诊断塞进去会制造假代码变化。应用报告需要区分 before/after 的上下文差异。
- coverage 继续说明文件在本次选项下可分析、缺失或不受支持。测试文件被识别并排除后仍是受支持文件处理结果，不标成 unsupported-language；tests-only 比较应是可分析的空图，而非“不支持文件”。首版无需新增公共 coverage 状态才能表达这一点。
- [前端缓存](../../src/structure-review/use-structure-review.ts)，5–8、34–67 行目前只围绕 Review、Snapshot、includeTests 与 adapter 生命周期。工具/上下文策略在应用生命周期固定时可沿用；显式重分析必须替换旧结果。若允许运行中改变配置或工具，必须使旧缓存与迟到结果失效，不宣称已有自动失效能力。

## Q5：具名箭头与自身内容

固定规则只读取 `VariableDeclaration.kind == Const`、`VariableDeclarator.id == BindingIdentifier`，以及解开透明包裹后直接为箭头的 init。复用 Oxc `get_inner_expression()`；不使用运行时 function.name、函数类型或框架调用名称推断。依据与版本见 [草案 Q5](typescript-code-graph-plan.md#具名箭头与内联回调的最小判定)。

- 一个绑定只有一个 Callable；声明里的绑定名、显式类型、箭头参数、async/泛型/返回类型及类型断言由该对象按稳定约定保留。签名不含函数体，原始 source 保留完整声明。
- `map/filter/then` 的参数箭头不创建 Callable 或 ExecutionRegion。它们的参数、表达式顺序和函数体仍在最近 owner 的 code 中，内部调用/writes/引用仍归该 owner。
- 不因匿名回调没有节点就删除其调用观察；也不把构造回调解释成即时执行，不补到 owner 自身的虚假自调用。
- 匿名回调中显式声明的具名子对象仍按普通声明规则提取；这些子对象的自身内容从 owner 中移除。无需为“出现在回调内”再建立一套对象分类。
- `const fn = memo(...)`、factory、条件表达式、解构绑定、let/var 不由这条最小规则提升；保留普通声明及初始化内容。`const fn = () => () => work()` 只让外层直接绑定成为 Callable。
- 透明包裹只用于分类：`as`/`satisfies` 内容仍参与正常自身内容比较。匿名回调无独立 NodeId，不能用其 LSP hierarchy item 制造第二个目标身份。

公共 own-content 边界已经存在于 [node.rs](../../src-tauri/src/structure_review/code_graph/node.rs)，29–43 行；可借鉴 [Rust owner 选择与子内容剔除](../../src-tauri/src/structure_review/rust_analyzer/syntax.rs)，109–177 行，具体归一化不能复制 Rust token 规则。

## Q6：JSX 使用关系

复用 `CodeReference.semantic: String`，以 `jsx.uses_component` 等明确语义表达源码中的组件使用。它从最近建模 owner 指向已验证组件声明，目标可以按其实际声明是 Callable、Type 或 Value；不能为连边强制把所有大写绑定提升成 Callable。

- AST 记录 JSX occurrence，并按实际解析模式判断 intrinsic 与组件表达式。对 `<div>`、`<button>` 等 intrinsic 标签完全不创建 HTML 节点或组件边；标签、属性、文本和嵌入表达式继续参与原文/内容处理。
- `<Card />`、`<UI.Card />` 的目标通过服务定义与本地声明索引核验；不可仅凭大小写或字符串拼接猜目标。最终 Node/External/Unresolved 继续遵循公共契约，图外目标继续被 UI 隐藏。
- outgoing call hierarchy 若包含 JSX 使用 occurrence，按源码位置改为组件使用关系；不能同时再生成同一 occurrence 的普通调用边。真实 `Card()` 调用仍是 `code.calls`，同一个 owner 同时包含两种用法时可同时保留两种语义。
- `<Card value={load()} />` 中的 `load()` 仍是普通调用。`<button onClick={() => save()} />` 不产生 button 关系，但保留 save 的语法调用观察及其能证实的目标。
- 同一 owner 多处 `<Card />` 在公共图中是一个高层关系事实；不能让重复次数或行号进入差异。替换成 `<Other />` 产生旧组件边删除、新边增加。

现有 [关系显示](../../src/structure-review/graph-presentation.ts)，274–303 行和 [详情格式化](../../src/components/structure-review/structure-details.tsx)，72–75 行可以传递开放语义。应验收真实布局/详情，而非为 JSX 建第二套画布。`.js` 中 JSX 的 parser/服务模式对齐也要验证，见 [JS/JSX 参考](typescript-vscode-reference.md)。

## Q7：跨文件声明与重载

Q7 已选源码声明分别显示；不要把 TypeScript 的 Symbol 身份直接当 CodeNode 的源码声明身份。

- `a.ts` 与 `b.ts` 对同一 interface/namespace 的声明，各有稳定 ID、各自 source、各自成员 contains。修改 b 的字段只修改对应成员；不能因 semantic symbol 合并而把 a 的自身内容改掉。
- 多个 LSP 定义位置要映射为已确认的声明候选，保留其来源。成员查询能够确认具体声明时准确落到该声明；指向合并符号整体的查询不能任挑第一份，也不能将已确认的全部声明统称 Unresolved。
- 公共 `ReferenceTarget` 一条引用只有一个目标，但节点允许多条同语义引用。若某关系的静态意义确实覆盖多份声明，可用既有引用集合表达；不能把普通调用的多个候选当成多个确定运行目标。最终映射规则须由 interface/namespace/member/alias fixture 固定。
- 只选 a 时，b 不进入节点表；被确认的 b 目标使用 External。同样源码在两侧不同 checkout 根的身份一致；添加 b 不使 a 获得新身份。
- 前端保持独立卡片/成员归属，不按名称合卡；`code.member_of` 只能改变经证实的归属，不能将同名声明片段吸附到首个 Type。当前 [typeTarget](../../src/structure-review/graph-presentation.ts)，94–117 行只接收唯一目标，必须保留完整候选后再作判断。
- 当前 source 只是原文，公共图没有源码路径/多位置导航。Q7 最低验收是每份声明有独立对象且详情对应各自原文；若为了区分同名卡片附上文件提示，应由适配器/应用提供不参与 diff 的展示信息，不能让前端解码 opaque NodeId，也不能把路径展示需求塞入语义 extensions。
- 同 owner 函数 overload 按草案工程建议聚合成一个 Callable，保留全部签名顺序及实现；将 LSP 返回任一重载签名的位置映射到同一源码对象。不要把这个局部聚合扩展成跨文件合卡。

公共 [单一 placement](../specs/0018b-code-relationships.md#containment-through-node-ids) 保持不变；它本身没有替用户决定按片段还是按符号聚合，不能将 Q7 描述为被公共模型强迫的结果。

## Q8：测试文件筛选

在适配器选中文件处理里按仓库相对路径和已支持扩展名识别：文件基名含约定的 `.test.` 或 `.spec.` 后缀段，或路径具有完整 `__tests__` 目录段。具体匹配器必须在 fixture 中固定大小写与声明文件边界；不能因为路径包含 `test` 子串就排除。

- `include_tests=false` 时，已识别测试文件的节点、自己的内容和 outgoing references 均不进入输出图；若服务返回这些范围的调用层级，也必须过滤，不能把测试调用迁移给业务 owner。
- 测试源码可以继续作为受控的同侧语义上下文；图成员筛选与服务上下文分开。不要为了关掉测试展示去修改真实 tsconfig、删除工作区文件或改写送往服务的原文。
- 业务文件中普通的 `test()`、`it()`、`describe()` 及自定义别名不能触发排除；`shared.ts` 不因测试引用它而消失。
- 开启开关时恢复本次所选测试文件的支持对象与关系，而非额外扩充到整个测试工程。两侧使用同一个值；重试保留值，新 Snapshot 恢复默认，沿用已有 [前端开关契约](../specs/0020-structure-changes-review-view.md#test-code-analysis-switch)。
- 若某侧全部候选文件都被测试规则排除，可在完成必要输入校验后返回空图；不必为零个输出对象启动语义会话。错误读取/非法路径不能由“测试已排除”掩盖。

## 最小实施顺序

| 顺序 | 交付内容 | 进入下一步的依据 |
| --- | --- | --- |
| 1 | 固定 TS 工具入口、原文/URI/项目输入、范围控制与生命周期的小型真实服务验证 | TLS 包装层仍使用预期项目；仓库调用可解析；第三方不进入默认项目；进程可回收。保存确切输入、版本、结果与未覆盖范围 |
| 2 | TS 适配器内部提取：文件模式、Q5–Q8、稳定身份、own content 与引用 occurrence | 使用公共图与 diff 验证内容边界；不以节点数量正确替代身份/父内容检查 |
| 3 | 语义查询映射及每侧报告 | aliases/re-exports、JSX、重载/合并声明、writes/calls 的有证据目标正确；缺依赖与硬失败区分；诊断不参与代码差异 |
| 4 | 应用路由、每侧组合、coverage、冻结配置和取消 | 混合 Rust+TS+JS、空侧、跨扩展名改名、某语言失败可通过真实执行 seam 验证；最终图经公共校验 |
| 5 | 投影/详情/文案和缓存衔接 | JSX 与多声明独立显示；测试切换及 stale result 规则保持；运行说明可读，详情原文正确 |

步骤是实现依赖顺序，不要求先做一套完整通用插件注册框架或持久化语义缓存。Rust 参数与 TS 工具配置在各自适配器内部，应用只负责选择、冻结、组合和报告。

## 行为验收矩阵

下列均为后续验收要求，本次文档核查没有执行。fixture 应通过模块公开接口或实际应用执行 seam 观察结果，不只测试实现内部的匹配函数。

| 编号 | 输入或操作 | 必须观察到的行为 | 层次 |
| --- | --- | --- | --- |
| A1 | 同一 Snapshot 同时改 `.rs`、`.ts`、`.js`，TS 调用选中 JS 声明 | 两侧最终图保留各语言节点，TS→JS 为 Node；未推断 TS→Rust 边；diff 经公共图计算 | 应用 + 真实服务 |
| A2 | 新增/删除 TS 文件，`.rs → .ts`、`.ts → .js` 改名，普通文件 → symlink/gitlink | 分侧路由准确；absent 与 unsupported-kind 正确；不能把旧路径交给新侧分析器；语言/路径变化允许增删 | 应用 |
| A3 | 只修改测试约定文件且默认排除，再开启 includeTests | 初次是受支持的空差异；开启后恢复测试对象和改动；无需 coverage 状态伪装“不支持” | 适配器 + UI |
| A4 | `widget.test.tsx`、`parser.spec.mts`、`__tests__/x.js`、`contest.ts`、业务文件中 `test()` | 前三者按约定排除，后两者保留；目录段/扩展名边界准确 | 适配器 |
| A5 | `const load = () => xs.filter(x => x.ok).map(x => format(x))` 修改 map 回调 | 只生成 load 这个箭头 Callable；load 自身内容修改；内部 format 调用保留，匿名回调没有节点 | 图 + diff + 真实服务 |
| A6 | `const load = (() => work()) satisfies Handler`，以及 memo/factory/条件/解构/let/var 反例 | 包裹后的直接 const 箭头被识别；其他写法不由最小规则提升；显式类型变化不因分类解包被丢弃 | 图 + diff |
| A7 | inline callback 内有独立 `function helper`，只改 helper 的 body；另只改 callback 表达式 | 前者只改 helper own content，后者改最近 owner；原文详情保留完整源码；不产生虚假 owner 自调用 | 图 + diff |
| A8 | `<Card value={load()} />` 与 `<button onClick={() => save()} />` | Card 为组件使用关系，load/save 为正常调用观察；button 无节点/组件边；属性/文本改动仍修改 owner 内容 | 图 + 真实服务 |
| A9 | 同一 owner 包含两次 `<Card />` 和一次 `Card()`；将其中全部 JSX 使用改为 Other | 同种组件使用去重，组件边与普通调用分开；修改 JSX 不误删真实 Card()；旧组件边删除、新边增加 | 图 + diff |
| A10 | a.ts/b.ts 两份同名 interface 或 namespace，只修改 b 的一个成员 | 声明分别显示；b 成员单独修改，a 内容不变；包含树仍合法；LSP 多声明不取第一个了事 | 图 + 真实服务 + UI |
| A11 | A10 只选择 a，再把 b 纳入同一图；同 owner overload 增删/换序 | 原有声明 ID 不变；b 按范围为 External/Node；overload 保持单 Callable，签名变化可比较 | 图 + diff |
| A12 | 缺业务依赖与本地跨文件调用同时存在；另制造服务启动失败/协议错误/超时 | 前者保留本地结构与可靠关系、Unresolved 和应用说明；后三者整次失败，不切语法成功 | 真实服务 + 应用 |
| A13 | 第三方包确实已安装、仓库 workspace package 经包名导入、基础库 Promise/Array 包装 | 第三方源码/声明不进入默认项目；第一方 package 不因包名被排除；标准库帮助定位本地目标。验证项目输入，不只看 UI | 真实服务 |
| A14 | 中文路径、空格、emoji、CRLF、源代码含非 BMP 字符；两个 checkout 根相同源码 | URI 与 UTF-8/UTF-16 换算正确；定义落在真正声明而非邻近 token；稳定 ID 一致 | 协议 + 适配器 |
| A15 | 两个文件连续产生运行诊断，或两侧项目配置不同 | 每条说明归属正确，不被“最后一条通知”覆盖；配置差异按本侧真实配置处理；只变说明不制造 node 内容 diff | 真实服务 + 投影 |
| A16 | before/after 任一语言硬失败或 panic，同时另一侧运行；用户取消、丢弃 future、刷新/关闭 | 同组工作被停止并等待；Node/TLS/tsserver 被回收；before 清理完成才释放工作区保护；根因不被取消覆盖 | 应用 + 进程 |
| A17 | 先有结果，再切测试选项、重试、替换 Snapshot，旧请求最后返回 | 旧结果不能覆盖新请求，重试保留选项，新 Snapshot 恢复默认；版本/配置若可变须验证额外失效 | 前端生命周期 |
| A18 | 只有图外/Unresolved 边变化、TS import/export 改动、同名多声明和长 JSX 源码 | 延续图内关系显示政策；不误用 Rust use 隐藏条件；详情来源准确；宽窄窗口和长文本可用 | UI |

语法与内容 fixture 还应包含 ASI、模板/正则字面量、JSX 有意义空白、成员重排、默认导出和 JSDoc 原文。JSDoc 不增加专用比较预期；验证现有通用流程没有主动屏蔽它，也不承诺修改 JSDoc 后关系恒定。

## 仍属工程验证的事项

下列是通用实现的验收边界。已有 scope、Node、references、JSDoc 和正常退出的代表性
LSP 实验见 [结果报告](typescript-lsp-validation.md)；无需重做相同事实调查，但必须扩展到真实适配器行为。

- TLS 包装后 didOpen 的项目归属、完整第一方 roots 的发现、多 tsconfig/references、同文件多项目冲突、无配置 JS 默认值。
- TS5/6 配置、Node ESM/CJS、paths/exports/imports 与第一方 workspace package 在固定快照下的解析；缺失/陈旧声明产物不可从当前 checkout 偷补。
- 默认基础库与 DOM/Node 运行时声明的来源、固定版本及允许闭包；标准库替换入口与自动 types/ATA 不得扩大业务第三方范围。
- Oxc 与语义服务的 TS/JS/JSX 模式对齐、语法版本支持、错误恢复。恢复 AST 不自动等于完整提取成功；所选源码无法满足承诺时明确报错。
- Q7 多声明目标的关系语义和源码归属、Q5 箭头绑定的 LSP 位置、Q6 JSX occurrence 去重与重分类。
- 共享 LSP 必需的反向请求、诊断汇总、退出行为、所有子进程的回收，以及峰值资源消耗；不能从 parser 微基准推断端到端时延。

这些验证由实现承担。只有具体失败证据表明必须改变已接受产品范围时，才带样例回到产品讨论。

## 验证落点与本次检查

可扩展已有 [应用并行测试](../../src-tauri/src/structure_review/application/tests/parallel.rs)、[工作区执行测试](../../src-tauri/src/structure_review/application/tests/workspace_execution.rs)、[图契约测试](../../src-tauri/src/structure_review/code_graph/tests/graph.rs)、[diff 测试](../../src-tauri/src/structure_review/code_graph/tests/diff.rs)，TS 私有测试按仓库规则放入适配器的 `tests/` 目录，不嵌入生产文件。前端已有 [图投影测试](../../tests/structure-graph-presentation.test.ts)、[生命周期测试](../../tests/structure-review-lifecycle.test.tsx) 和 [视图测试](../../tests/structure-changes-view.test.tsx)。

实现后的验证遵循仓库 Cargo 全局锁与一次全量测试节奏；前端布局/交互使用 Preview Harness 作真实渲染检查，不能代替测试、类型检查、Lint 和构建。自动安装和正式发行包不是这份清单的完成门槛；不手动触发 quality-gate。

本次仅创建本文并检查链接和文本格式；未修改生产源码、依赖或其他文档，未运行编译、服务探针或上述验收。

# TypeScript 与 JavaScript Code Graph 的解析器比较

调研日期为 2026-09-14。本篇比较 Oxc 与 Tree-sitter 作为 Narrant 语言 Adapter 内部的语法组件，并回答是否将 JS/JSX 与 TS/TSX 同批支持。本文是访谈事实底稿和工程建议，未形成 ADR，也没有修改依赖或运行解析器性能测试。

版本化核查采用 Oxc `crates_v0.150.0`、Tree-sitter core `v0.27.0`、JavaScript grammar `v0.25.0`、TypeScript/TSX grammar `v0.23.2`。本次读取了这些 tag 的官方源码。Oxc 0.150.0 于调研当日发布；滚动 docs.rs 页面在调研期间先后返回 0.149.0 和 0.150.0，因此以下接口细节以明确 tag 为准。版本记录不是要求立即采用最新版本。[Oxc 发布](https://github.com/oxc-project/oxc/releases/tag/crates_v0.150.0)、[Tree-sitter 发布](https://github.com/tree-sitter/tree-sitter/releases/tag/v0.27.0)、[JS grammar 发布](https://github.com/tree-sitter/tree-sitter-javascript/releases/tag/v0.25.0)、[TS grammar 发布](https://github.com/tree-sitter/tree-sitter-typescript/releases/tag/v0.23.2)。

## 当前建议

用户本轮明确偏向 LSP + Oxc，默认只支持仓库内与标准库；JSDoc 已有能力自然保留，不增加特殊适配。当前主方案因此是“语义服务 + Rust 侧语法提取”；语义服务既可以是传统 tsserver 的 LSP 包装，也可以是原生 TS7，不能把 LSP 等同于后者。Compiler API 只保留为需要更直接数据访问时的备选。依赖范围、VS Code 架构和新的服务协议探针见[后端调研](typescript-code-graph-backends.md)、[方案验证](typescript-code-graph-plan.md)和[VS Code 参考](typescript-vscode-reference.md)。

建议将 `.ts`、`.tsx`、`.js`、`.jsx` 的显式源码对象提取放在同一个 Adapter 的首轮范围内，共用对象映射、own content 处理和关系提取。两套候选解析器都已经覆盖这四种输入；在 TS/TSX 实现上增加 JS/JSX 的文件分派与多数 AST 遍历，属于相对小的工作。必须保留各文件的实际解析模式，并增加 JavaScript 项目语义的专门验收，不能把“已接受四种扩展名”作为完整支持的证明。[Oxc SourceType](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_span/src/source_type.rs#L192-L216)、[Tree-sitter JavaScript](https://github.com/tree-sitter/tree-sitter-javascript/tree/v0.25.0)、[Tree-sitter TypeScript](https://github.com/tree-sitter/tree-sitter-typescript/tree/v0.23.2)。

如果最终采用“Rust 侧语法提取 + TypeScript 语义服务”的后端路线，优先验证 Oxc。依据是 typed AST、局部绑定/引用分析和 JS/TS printer 与当前需求较贴近，而非未经测量的速度排名。Tree-sitter 仍是成立的备选，尤其适合需要跨多语言统一查询、保留具体语法结构或频繁编辑后增量更新的产品。若最终改为直接使用 TypeScript Compiler API 遍历 AST，则应先验证其 AST 能否完成全部提取，避免为同一职责引入第二套 parser；后端路线另见[分析后端事实调研](typescript-code-graph-backends.md)。

## 四种文件共用什么，又必须区分什么

| 输入 | 基础语言 | JSX 语法 | Oxc 0.150.0 的扩展名模式 | Tree-sitter 的官方 grammar |
| --- | --- | --- | --- | --- |
| `.js` | JavaScript | Oxc 默认关闭；参考 VS Code 常规语言服务需显式开启 | JavaScript + Standard，需配置 JSX | JavaScript grammar；这套 grammar 本身也接受 JSX |
| `.jsx` | JavaScript | 开启 | JavaScript + JSX | JavaScript grammar |
| `.ts` | TypeScript | 关闭 | TypeScript + Standard | TypeScript grammar |
| `.tsx` | TypeScript | 开启 | TypeScript + JSX | TSX grammar |

Oxc 用 `SourceType` 分开表示语言、JSX 和模块模式。其 `from_path` 将四种普通扩展名初始化为 `Unambiguous`，根据 ESM 语法识别 Module 或 Script；`.mjs`/`.mts` 明确为 Module，`.cjs`/`.cts` 为 CommonJS。这个便利函数不读取 `package.json`，因此不能代替项目语义后端对模块系统的判断。Tree-sitter 的 TS grammar 继承 JS grammar，并为 TS 与 TSX 提供两个入口；JS 与 JSX 共用另一个 grammar。这些都是库的输入方式，不能自动决定 Narrant 的支持范围。[Oxc 映射源码](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_span/src/source_type.rs#L192-L216)、[Oxc from_path](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_span/src/source_type.rs#L607-L628)、[TS grammar 继承](https://github.com/tree-sitter/tree-sitter-typescript/blob/v0.23.2/common/define-grammar.js#L1-L5)、[TS/TSX Rust 入口](https://docs.rs/tree-sitter-typescript/0.23.2/tree_sitter_typescript/)。

TypeScript 与 JavaScript 共享大量运行时代码语法，所以函数、类、变量、调用、赋值和 JSX 的提取通常能复用。TSX 与 JSX 的解析模式仍然不同，TSX 允许类型语法，JSX 不应因此接受 TypeScript 标注；`.ts` 与 `.tsx` 也有歧义规则差别，例如 `.ts` 的尖括号类型断言 `const value = <Item>input` 在 `.tsx` 中被禁止，需写成 `input as Item`。因此不能统一把全部文件交给最宽松的 TSX 模式。[TypeScript JSX 手册](https://www.typescriptlang.org/docs/handbook/jsx.html#the-as-operator)、[Oxc SourceType](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_span/src/source_type.rs#L197-L214)。

本轮用户要求参考 VS Code 的支持程度。其随附 TS6.0.3 源码把 `.js` 识别为 ScriptKind.JS，并将 JS 和 JSX 两种 kind 都设为 JSX language variant。因此接受 `.js` 内 JSX 是语法层对齐事项，需要给 Oxc 显式开启 JSX，不能沿用 `from_path(.js)` 默认值。此处不承诺 React 类型完整，也不意味着 Node 直接支持 JSX 执行。[TS6 扩展名与语言变体](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/compiler/utilities.ts#L8948-L8951)、[VS Code 参考](typescript-vscode-reference.md#jsjsx-与项目上下文)。

项目语义也有差别。`allowJs` 控制 JS 文件是否进入 TypeScript 项目，`checkJs` 控制是否报告 JS 检查错误，两者不是同一个开关；`jsconfig.json` 用来描述 JavaScript 项目。JS 检查另有属性赋值推断、构造函数/prototype、CommonJS、开放对象等规则。沿用 TypeScript 服务可以复用其中很多能力，Adapter 仍需把返回的声明与自己的源码对象对应起来。[allowJs](https://www.typescriptlang.org/tsconfig/allowJs.html)、[checkJs](https://www.typescriptlang.org/tsconfig/checkJs.html)、[jsconfig](https://code.visualstudio.com/docs/languages/jsconfig)、[JS 检查差异](https://www.typescriptlang.org/docs/handbook/type-checking-javascript-files.html)。

## Oxc 与 Tree-sitter 的工程差别

| 维度 | Oxc | Tree-sitter | 对 Narrant 的影响 |
| --- | --- | --- | --- |
| 树的形态 | 面向 JS/TS 的 AST，有 typed enum/struct，明确区分 `BindingIdentifier`、`IdentifierReference`、`IdentifierName` | 通用 CST；Rust 核心接口是 `Node`、kind、field、children、byte range | Oxc 可以直接按语言构造匹配声明与表达式；Tree-sitter 需要维护 grammar 到 Code Object 的映射。[Oxc AST](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_ast/src/ast/js.rs)、[Tree-sitter Node](https://docs.rs/tree-sitter/0.27.0/tree_sitter/struct.Node.html) |
| Rust 遍历与查询 | typed visitor/AST enum，字段与节点形态由 Rust 类型表达 | TreeCursor/Node 遍历和 S-expression Query；官方提供 `node-types.json`，可据此生成额外 typed 包装 | Oxc 少一些运行时字符串判断；Tree-sitter query 易于集中维护，但 query/schema 仍需版本化测试。不能断言 Tree-sitter 无法加 typed API。[Oxc visitor](https://github.com/oxc-project/oxc/tree/crates_v0.150.0/crates/oxc_ast_visit)、[Queries](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html)、[Static Node Types](https://tree-sitter.github.io/tree-sitter/using-parsers/6-static-node-types.html) |
| token、trivia、注释 | `Program` 保留 source text、spans、comments、directives、hashbang；可开启 `TokensParserConfig` 收集源码顺序 token，默认不收集 | CST 保留 named/anonymous 语法节点；JS grammar 把 comment、html_comment 和空白列为 extras | 两者都可用范围截取原文。CST 不意味着每一段空白都单独成为可遍历节点；AST 也不意味着注释和原文都丢失。[Oxc Program](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_ast/src/ast/js.rs#L43-L69)、[Oxc token 配置](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_parser/src/config.rs#L8-L75)、[JS extras](https://github.com/tree-sitter/tree-sitter-javascript/blob/v0.25.0/grammar.js#L27-L31) |
| 代码打印与归一化 | 同生态 `oxc_codegen` 打印 JS 和 TS，可以控制注释、引号、缩进等；它与 transformer/minifier 是不同组件 | 核心接口提供树和源码范围；规范化 JS/TS 输出需要本项目的投影逻辑或另一个 printer | Oxc 减少基础 printer 工作，但两者都没有 Narrant 的“去除独立子对象 own content”规则。`source` 应始终来自原文，不由 printer 重新生成。[Oxc codegen](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_codegen/src/lib.rs#L69-L85)、[codegen options](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_codegen/src/options.rs)、[Node 源文接口](https://docs.rs/tree-sitter/0.27.0/tree_sitter/struct.Node.html#method.utf8_text) |
| 局部 scopes、symbols、读写 | `SemanticBuilder::build(&Program)` 创建该 Program 的 Scoping、bindings 和 references；ReferenceFlags 区分 Read、Write、Type 等 | highlighting 的 locals query 可声明 scope/definition/reference，并在作用域里做名称匹配 | Oxc 更接近需要的绑定分析；Tree-sitter locals 是有用基础，但不能当作完整 JS/TS binder 或 checker。[Oxc builder](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_semantic/src/builder.rs#L62-L103)、[ReferenceFlags](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_syntax/src/reference.rs#L33-L134)、[Tree-sitter locals](https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting.html#local-variables) |
| 跨文件与类型语义 | 上述本地 SemanticBuilder 接受一个 Program，不是 TypeScript 项目 TypeChecker；局部 import binding 不自动等于被导入实现 | parser/Query 同样不提供 TypeScript 跨文件类型检查 | 两者都需要另一个可靠语义后端确认导入目标、成员定义、继承与调用签名；选择 Oxc 不消除这部分成本。此判断限定于上述组件，不否认 Oxc 生态存在 resolver 等其他工具。[Oxc build](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_semantic/src/builder.rs#L304-L309)、[Tree-sitter 查询机制](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html) |
| 错误恢复 | 部分语法错误可恢复并给出 AST/diagnostics；致命错误返回空 program。仅看 AST 存在不够，还要启用 semantic syntax checks | 用 ERROR 节点和零宽 MISSING 节点表示恢复结果，可继续查询树 | 恢复能力便于定位失败，不能直接把部分树当作完整图比较；Narrant 的选中文件提取失败仍应报错。[Oxc ParserReturn](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_parser/src/lib.rs#L135-L205)、[ERROR/MISSING](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html#the-error-node) |
| 增量 | 当前公开 Parser 接口接收一次 source text；本次未发现接受 old tree/edit 并复用子树的对应入口 | 标准工作流是 edit old tree，再 parse 并传 old tree，共享结构 | Narrant 当前每侧固定快照、新会话分析，不是每键更新。Tree-sitter 的增量能力有价值，但目前不是首要选型收益。[Oxc Parser](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_parser/src/lib.rs#L293-L321)、[Tree-sitter Editing](https://tree-sitter.github.io/tree-sitter/using-parsers/3-advanced-parsing.html#editing) |
| 新语法与维护 | 一个 JS/TS 工具链持续修复 parser；0.150.0 就包含 accessor、using、JSX、TS overload 等边界修复 | JS 与 TS grammar 分别发布；JS README 明说极新语法可能未覆盖，TS grammar tag 内已有 satisfies、decorator 等规则 | 不能据此给出“永久完整支持”或简单速度排名。应锁版本，验收目标仓库实际用到的新语法。[Oxc changelog](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_parser/CHANGELOG.md#01500---2026-09-14)、[JS README](https://github.com/tree-sitter/tree-sitter-javascript/tree/v0.25.0)、[TS grammar](https://github.com/tree-sitter/tree-sitter-typescript/blob/v0.23.2/common/define-grammar.js) |

## Narrant 需要自己实现的规则

Q5 已按用户“区分简单则保留具名箭头”的条件收敛。最小规则只检查 const 声明、
`BindingIdentifier` 以及直接初始化的 `ArrowFunctionExpression`；Oxc 的
`get_inner_expression()` 已能解开括号/as/satisfies 等透明包裹，不穿透调用或条件表达式。
因此 `const fn = () => xs.map(x => x.id)` 只产生 fn 一个箭头 Callable，map 的回调仍内联。
这个分类不需要 checker，也不需要为函数式链式调用维护方法名表。
[变量声明字段](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_ast/src/ast/js.rs#L1185-L1229)、
[透明包裹读取](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_ast/src/ast_impl/js.rs#L229-L250)。

这是低复杂度的语法分类判断；完整适配器仍需实现通用的自身内容投影、身份和位置映射。
本轮额外用已有 TS5.9.3 parser 验证了五个语法样例，结果见
[语法验证记录](typescript-code-graph-plan.md#箭头分类的语法验证)；没有把它当作 Oxc 运行测试。

现有 Code Graph 以 `signature`、`code` 保存规范化 own content，以 `source` 保存完整原文；独立子对象的修改不能递归标记父对象自己的内容。NodeId 也必须跨 checkout 稳定。这些规则由 [CodeNodeData](../../src-tauri/src/structure_review/code_graph/node.rs) 和 [LanguageAnalyzer](../../src-tauri/src/structure_review/code_graph/analyzer.rs) 约定，任何 parser 都不会自动生成符合它们的 Code Graph。

对 Oxc 的具体采用方式建议如下，属于尚需原型验证的工程设计。

1. 先按文件的实际语言、JSX 与模块上下文解析，保留原文与范围。源代码 `source` 直接截取，避免打印器改变注释或排版；解析诊断和需要的早期语法检查必须通过。
2. 用 typed AST 识别函数、绑定的 arrow/function expression、class/interface、字段、访问器、imports/exports 等，再按已经确认的对象粒度分配稳定 ID。Oxc 的 NodeId、SymbolId 是单次 AST/语义表索引，不能直接替代 Narrant 的跨快照 NodeId；Tree-sitter Node 的运行时 identity 同样不应替代。
3. 按对象 ownership 投影 signature/own code，再使用固定打印规则或语法感知的 token 投影归一化。不能把整个 class/function 子树直接打印成父对象 own code。需要保留类型标注与 JSX，不能先做 TS 擦除、JSX lowering 或名字压缩；它们会改变正在审查的源码对象。
4. 从 CallExpression、NewExpression、赋值/更新表达式等提取候选关系及 owner。Oxc 本地 binding 可正确区分遮蔽的同名变量，减少猜测；成员调用、导入链、返回值和类型决定的目标仍交给 TypeScript 语义后端。仅仅查到变量的 binding 不证明该变量运行时总是指向一个 callable。
5. `code.writes` 需要区分变量重赋值和对象属性修改。Oxc `MemberWriteTarget` 将 `obj.field = value` 中的 `obj` 标为读取接收者，不能据此直接生成“写 obj 变量”的边；属性目标仍需语法位置和定义身份。普通 Read flag 也不能直接变成 `code.calls`。[ReferenceFlags 的精确定义](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_syntax/src/reference.rs#L80-L134)。

归一化尤其需要检查 ASI 的换行含义、正则/字符串、tagged template 的 raw 文本、JSX text 和 directives，不能用正则去注释后统一压缩空白。Oxc codegen 提供相关语言打印能力，但其选项本身可能改变字面量表示，甚至某些可观察值，例如 `ascii_only` 文档说明对正则的转义会改变 `RegExp.prototype.source`。应冻结适合审查的设置并用成对 fixture 验证，不将“成功打印”当作 own content 语义等价证明。[CodegenOptions](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_codegen/src/options.rs)。

用户已澄清 JSDoc 的成本边界：服务已有的解析和推断能力保留，不安排特殊适配。`@type`、`@param`、`@returns`、`@template` 等原文仍送到语义服务；本项目不自行重写其规则，也不预处理源码来抹去它们。首版不增加专用契约模型或差异流程；通用提取和查询已经提供的信息可以使用。若注解改变目标解析，关系仍可能变化。[TypeScript JSDoc Reference](https://www.typescriptlang.org/docs/handbook/jsdoc-supported-types.html)。

## 同批加入 JavaScript 的相对成本

下表以“TS/TSX Adapter 已实现显式源码对象和选定关系”为基线。相对复杂度用于区分工作来源，不是工时估计或已完成验证。

| 工作 | 相对增量 | 原因与边界 |
| --- | --- | --- |
| `.js`/`.jsx` 文件分派、语言标记、复用函数/类/变量/调用 AST 遍历 | 小 | 两个候选库都已覆盖；无需第二套 Code Graph 模型，仍需四模式 fixture |
| 现代 JS/JSX 的对象粒度、own content、基本局部 calls/writes | 小到中 | 大量语法共用；无类型标注时的空签名部分、JSX 文本、arrow owner 和字段写入需要验证 |
| JS 项目成员范围 | 中 | 尊重 `allowJs`、`checkJs`、`jsconfig` 和混合项目；选中文件未进入配置项目时的处理需沿用后端的明确策略，不能悄悄强改仓库配置 |
| ESM/CJS、`.mjs`/`.cjs`、`package.json` 模块边界与互操作 | 中到高 | 仅靠扩展名或 import 文本不足；JS 的 require/module.exports 语义与 AST 中的调用/赋值形态需要正确对应 |
| JSDoc | 保留已有能力，不安排专用适配 | TypeScript 服务自然利用原文中的注解；不增加专用契约模型和差异流程 |
| prototype、构造函数式 class、动态挂载属性的完整对象重建 | 高 | 官方 JS checker 能理解部分模式，但高层图节点边界、跨语句 ownership 和稳定身份并非 parser 免费提供；可先按显式语句/函数范围比较，不能声称还原任意运行时对象 |
| 基于 any、计算属性、eval、运行时工厂的完整调用目标 | 无法给出普遍完整承诺 | 动态目标应保留 Unresolved；增加 parser 不会自动获得运行时事实 |

项目范围的来源是 [allowJs](https://www.typescriptlang.org/tsconfig/allowJs.html)、[checkJs](https://www.typescriptlang.org/tsconfig/checkJs.html) 与 [jsconfig](https://code.visualstudio.com/docs/languages/jsconfig)。CommonJS、prototype 和开放对象的不同规则见 [JS 检查差异](https://www.typescriptlang.org/docs/handbook/type-checking-javascript-files.html)；ESM/CJS 的模块检测与互操作需按语义后端核验，[TypeScript ESM/CJS 说明](https://www.typescriptlang.org/docs/handbook/modules/appendices/esm-cjs-interop.html)可作为验收依据。表中复杂度是由这些规则与 Narrant 映射职责推得，未做量化测量。

因此，同批支持四种文件是合理起点。首版应统一承诺“提取已约定的源码对象，并对有可靠语义证据的关系解析目标”，让现代 JS/JSX 自然受益；是否把 prototype 动态模式提升为额外的 class/field 对象，应另列具体 fixture 和映射决定，避免把四扩展名支持扩成完整动态 JavaScript 建模。

## 版本锁定与验证边界

Oxc 的 Rust crates 仍是 0.x，0.149.0 changelog 有实际破坏性变更，例如 ParserReturn 字段重命名；0.150.0 workspace 声明 MSRV 为 Rust 1.96.0。采用时应一起锁定 parser/AST/semantic/codegen 的兼容版本并让 Cargo.lock 保留完整解析结果。Tree-sitter 需要同时锁 core 与 grammar；官方 Rust `set_language` 会检查 language ABI，grammar 的 node shape/query 也要随版本验收。较旧的 grammar 发布日期本身不证明功能不够，最新 tag 本身也不证明满足本项目语料。[Oxc breaking changes](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/crates/oxc_parser/CHANGELOG.md#01490---2026-09-07)、[Oxc MSRV](https://github.com/oxc-project/oxc/blob/crates_v0.150.0/Cargo.toml#L18)、[Tree-sitter set_language](https://docs.rs/tree-sitter/0.27.0/tree_sitter/struct.Parser.html#method.set_language)。

建议原型使用同一组固定 before/after fixture 验证：四模式和模块上下文；class/arrow/访问器/overload 的对象映射；独立成员修改不污染父内容；注释/排版变化与 JSX text 的区分；遮蔽绑定、import alias、函数值与真实调用；变量写入与属性写入；解析错误；跨 checkout 的相同 ID。JSDoc 不做契约映射，语义服务仍接收完整原文。若需比较性能，应在同一语料和图输出要求下测解析、提取、语义服务、总耗时和内存，不能用项目首页 benchmark 推出 Narrant 的端到端收益。

本篇的 Oxc 结论来自官方文档和版本化源码核查；没有运行 Oxc 的上述 fixture，也没有测量速度或内存。TS5.9.3 的独立语法探针在方案文档中单独记录，未改变公共图契约。

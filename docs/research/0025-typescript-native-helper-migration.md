# TypeScript / JavaScript 原生 Helper 迁移证据

本记录对应规格 0025 的 Task 4；集成基线 `1fd3fa0`，本地退役基线 `75c47ca`。
依照更新后的 0025/0025a 与 AGENTS.md，完成本地 macOS ARM64 的真实包、公共图/diff、
隔离安装及 Review 验收后，旧 TS 链在开发分支退役。候选支持包 `1.1.0` 仍保持
`published=false`、`verifiedTargets=[]`；全部开发、审查和本地验证结束后再集中完成
macOS ARM64 + Windows x64 Actions，本地结果不能替代最终平台验收。

## 固定工具和边界

- Node 24.18.0、TypeScript 6.0.3、@types/node 22.20.1、undici-types 6.21.0。
- Helper ID `narrant-typescript`，版本 `0.1.0`，projection `typescript-source-v1`，profile `typescript-static-v1`。
- 一侧一次 NDJSON 请求。Compiler AST、LanguageService Program、类型诊断和定义导航在同一进程内；
  不启动 TLS/tsserver/LSP，不加载业务插件、不写项目、不运行用户代码、不下载项目依赖。
- 复用旧受限 workspace/config 文件系统的政策：第一方源码、必要 JSON 元数据、固定声明闭包。
  business node_modules 中的源码/声明不可读；包配置 JSON 可按既有政策提供 extends 元数据。
- 同步 Compiler API 在当前 helper 的单次 Worker 中运行；主线程处理 control/EOF，等待 Worker 回收。
  公共 Host/ProcessTree 保持唯一进程树所有权、超时和强制取消屏障；没有额外进程框架。
- 生产支持包工厂与 Review 入口已接通；测试可注入闭包，生产不再装配旧 TS fallback。

## 兼容性判断

身份编码按 Rust `form_urlencoded::byte_serialize`，空格是 `+`，中文是 UTF-8 百分号编码，
标点逐字节编码；没有直接使用 `encodeURIComponent`。身份中的仓库相对路径保留平台原生分隔符，
协议 evidence 路径保持 `/`。真实旧 Oxc 声明与新 helper 对比了类别、native_kind、名字、ID、source、contains；
夹具含 Unicode、空格、标点、TS/JS、重载、参数属性和嵌套声明。退役前逐项对照通过后，
23 个历史节点冻结于 `native_languages/tests/fixtures/typescript-historical-identity.json`；
`typescript_historical_identity_and_source_contract` 继续验证全部字段与节点数量，
Windows 只将身份路径的 `%2F` 换为 `%5C`，实际运行留待最终平台验收。

比较内容允许原生打印差别；字符串引号和可选尾逗号可归一化。比较仍保留完整参数、初始化器和函数体，
只有具名声明主体的独立成员被裁剪。稀疏数组空位、顺序、JSX 文本、模板 raw 文本、正则、ASI 与
嵌入声明的内容变化必须产生差异。source 直接取原始文本，含 BOM/JSDoc/CRLF；无效 UTF-8 不能成功提取。

导航继续用 `getDefinitionAtPosition`，不能以 checker 符号替代异构 union 的选择。checker 只验证 type-only
路由。多项目冲突、mutable/动态/不确定目标保留 Unresolved。跨选集文件和测试上下文使用 repository external，
固定标准声明使用 dependency external；已选择且未过滤的目标成为 Node。

早期语法错误沿用旧边界：解析错误、缺失 const 初始化器、非法 break、同文件重复 let 均失败；
跨文件脚本全局冲突和一般类型错误不阻止提取。固定 TS 6.0.3 的 `getSemanticDiagnostics(sf)` 先初始化 binding，
再读取该 SourceFile 的 `bindDiagnostics` 中 2451；不将跨文件 semantic 2451 误当作语法错误。

## 原 syntax/content 断言迁移

这组与 graph_service 分开记录。新位置：`native-helpers/typescript/tests/projection.test.mjs`、
`occurrences.test.mjs`、`helper.test.mjs`；测试在独立文件，生产模块不嵌测试。

| 旧断言主题 | 新保留的可观察断言 |
| --- | --- |
| `own_content_excludes_children_but_keeps_callbacks` | 字段/方法修改不污染类；匿名回调修改属于最内层具名对象 |
| `formatting_and_comments_do_not_change_diff` | 格式/JSDoc改变不改 signature/code；source 保留原文 |
| `meaningful_javascript_text_survives_normalization` | ASI、模板空格、正则空格、JSX 文本、tagged raw 文本差异 |
| `sparse_array_hole_positions_and_counts_remain_distinct` | 前/后空位、多个空位、空数组互异 |
| `embedded_declaration_edits_remain_in_complete_expressions` | 初始化器、基类表达式、参数、函数体、enum 中嵌入声明变化计入外层完整表达式 |
| `full_expressions_ignore_formatting_and_optional_trailing_commas` | 数组/对象/调用/new/方法参数的普通尾逗号不改内容 |
| content.rs 全部 6 例 | 成员裁剪、混合变量声明、未建模成员、嵌入表达式、尾逗号、完整 source 与位置 |
| 文件容器/顶层执行/namespace | file 不冒充语言 module；文件与 namespace 各一执行区；声明/执行包含树、子改动隔离 |
| 文件族/ambient/重载/合并片段 | 8 个源码后缀和 d.ts/d.mts/d.cts；签名合并、同名 interface 分片不合并 |
| 具名函数值/省略包装/回调 | 外部绑定身份、对象嵌套 binding 路径、private/static 方法；函数内部匿名与局部 Callable 不独立建模 |
| 数据成员/匿名类型/ComparisonView | 类字段、属性、index signature；泛型/参数/函数内部匿名数据不泄漏；state/类型变化仍改变函数 |
| 导出/模块 routes/require | 纯 routes 无节点，导出修饰符参与内容，执行表达式拥有执行区；graph 全集验证关系 |
| 属性/数组插入、删除、重排及相邻语句 | contains 变化与父内容变化分开；完整初始化器保留 spread/顺序/空位；类自身不受子列表扰动 |
| 写入与析构赋值 | 写目标无普通 reference；右侧默认值、函数读取及嵌套调用保留 |
| JSX/简写/private/计算键/限定类型 | intrinsic 规则、成员终端查询、函数值 shorthand、计算键调用、extends/implements/type_use |
| 动态复合被调对象/tagged templates | 完整查询起止位置去重；内外调用都保留，插值函数值读取不被强语义吞掉 |
| Unicode/混合换行位置 | 原生 UTF-16 AST 查询精确，CR/CRLF/U+2028/U+2029/非BMP；旧 byte↔LSP 往返仅属退役 transport |
| parameter_properties.rs 全部 6 例 | visibility/readonly/override 组合、直接类归属、source/默认调用/类型、显式字段身份、内容差异、重载/匿名类边界 |
| 测试过滤/无效输入/早期错误 | include_tests 两向、仓库相对判定、大小写、内外链接、覆盖来源、同/跨文件 let、非法 UTF-8/语法 |
| alias_metadata 的可见语言含义 | import/export/const alias/type-only/shadow/namespace/union 由 graph 全集验证；不保留 Oxc 私有绑定表结构断言 |

旧 LSP framing/session、Oxc 特有源码坐标转换与私有别名表的实现断言不迁成新通信框架；
公共 Host 已验证协议、图/evidence 校验和进程生命周期。所有有语言含义的目标/内容/归属断言继续验证。

## graph_service 全集

旧 `tests/graph_service.rs` 的 22 个完整测试保留原断言，迁入
`src-tauri/src/structure_review/native_languages/tests/typescript_graph.rs`，只更换真实分析器工厂和夹具入口：

- `graph_parameter_properties_keep_field_types_and_default_calls`
- `graph_unverified_relative_imports_do_not_abort_analysis`
- `graph_cross_language_alias_barrel_and_callbacks`
- `graph_file_modules_keep_merged_execution_calls`
- `graph_named_function_values_and_omitted_initializers_keep_calls`
- `graph_runtime_wrappers_node_modules_and_jsdoc`
- `graph_jsx_properties_heritage_and_function_values`
- `graph_assignments_omit_writes_and_keep_calls_and_function_values`
- `graph_missing_dynamic_ambiguous_and_type_only_are_unresolved`
- `graph_overloads_and_merged_fragments`
- `graph_type_only_alias_chains_do_not_become_runtime_calls`
- `graph_project_conflicts_keep_unknown_calls_and_local_targets`
- `graph_workspace_references_without_built_dist`
- `graph_test_membership_lazy_context_and_stable_external_symbols`
- `graph_diff_format_child_parent_and_inline_callback`
- `graph_dynamic_unknown_names_ignore_format_but_preserve_literal_values`
- `graph_unicode_default_namespace_alias_and_union_evidence`
- `graph_repository_ambient_functions_and_nonambient_signature_boundary`
- `graph_async_member_calls_keep_concrete_targets`
- `graph_heterogeneous_union_members_follow_definition_navigation`
- `graph_repository_relative_test_classification_ignores_checkout_ancestors`
- `graph_config_dir_templates_retain_local_call`

另有原生身份/source 对照、真实 CPU 取消和内部目录别名/逃逸测试。
缺少显式要求的真实包时测试失败，不静默跳过。

## 双平台执行入口

```sh
node scripts/build-language-support-packs.mjs --id typescript-javascript --target aarch64-apple-darwin --output native-language-support-packs
node scripts/verify-native-analyzers.mjs native-language-support-packs --language typescript-javascript
```

Windows 同入口使用 `x86_64-pc-windows-msvc`。`language-support-packs.yml` 的 language 选择已加入
`typescript-javascript`，其 SDK 列表为空，构建所需 Node/TS/声明均走固定 checksum 下载。
归档前后都执行跨 TS/JS 本地调用、标准声明与 coverage 的真实图 smoke。
归档测试覆盖公共 validator；不以 TLS 版本或 transpile 成功替代图验证。
公共生命周期、真实安装、生产工厂、Review DTO、公开 Host graph/diff 都在聚焦 runner 中串行执行。

## 物理别名对照

旧链：`typescript_analyzer/mod.rs` 的 selected 是词法路径 BTreeSet；
`project/files.rs::checked_path` 返回 canonical 路径；随后 `documents.insert(path, document)` 合并物理文件。
在只读旧 1.0.0 安装包执行最小对照：`impl/a.ts` 包含 `export function run() { return 1; }`，
`alias` 是指向 `impl` 的目录链接，同时选择 `alias/a.ts`、`impl/a.ts`。结果成功，恰有一个
`impl/a.ts` 文件节点和一个函数；聚焦测试 `task4_legacy_physical_alias_probe` 1/1 通过（4.19 s）。
临时探针未进入生产或测试入口，没有修改用户安装包。

经控制者确认，只为 TS profile 放开受控内部物理别名。helper 按 canonical 路径生成一份图，
每个输入路径仍有 coverage，origins 聚合所有未过滤来源。TS 图身份仍取 canonical 路径；Python 的词法模块
身份规则不改，其他既有原生语言继续拒绝物理重复。输入完全相同的相对路径和仓库外链接仍拒绝。
双平台回归使用 Unix 目录 symlink / Windows junction，包含普通别名、`__tests__` 别名、真实 test 文件、
include_tests 两种值、重复词法路径和外部目录链接。

## 性能样本

macOS 27.0 / Mac14,2 / ARM64 / 8 GiB，候选包 Node 24.18.0 / TS 6.0.3。
`native-helpers/typescript/tests/benchmark.mjs` 生成 40 文件、400 导出函数和 400 已解析函数调用，
每轮独立 Node 进程，断言 440 节点、400 Node 关系、40 文件 coverage。
`/usr/bin/time -l` 统计子进程峰值 RSS，JS 墙钟含进程启动、结果读取和断言：

| 轮次 | 墙钟 ms | 峰值 RSS bytes |
| --- | ---: | ---: |
| 1 | 656 | 312229888 |
| 2 | 608 | 284540928 |
| 3 | 571 | 333430784 |

中位墙钟 608 ms；本地热文件系统、小配置样本，不是旧链性能对比，不外推大型仓库或 Windows。

```sh
NARRANT_TYPESCRIPT_PACK=/absolute/extracted/pack node native-helpers/typescript/tests/benchmark.mjs
```


## 本机验证结果（集成候选）

- 原生 Node 语言/配置/workspace 测试：62 passed，0 failed，0 skipped。
- 原 22 个 graph_service 全集 + 身份/source 对照 + CPU 取消 + 别名/测试/配置短路：25 passed，0 failed。
- TS 物理别名准入变更后的公共 Host：24 passed，0 failed。
- 应用单测：28 passed，4 显式真实工具测试 ignored；支持包服务：9 passed，2 显式真实安装测试 ignored。
- 真实安装旧测试改接新包：1/1；真实 TS/JS Review DTO：1/1；生产工厂安装：1/1；公开 Host TS/JS graph/diff：1/1。
- 其余聚焦 runner：公共 ProcessTree 3 passed（1 通用内部进程夹具 ignored），原生应用回调5/5、工厂/平台11/11。
- 构建/归档前后真实图 smoke、`check --lib`、Cargo fmt、作用域 ESLint、`git diff --check` 通过；脚本测试36/36。

安装/Review/公共进程整组在别名和全测试短路收尾前已通过；最后改动后只重跑直接受影响的 Node、TS 图和公共 Host。
最终候选包已重建。坏 tsconfig 不阻止空选集或全部词法测试排除；混合选集仍正常验证配置，越界始终先失败。
完整日志和精确本机命令在 ignored Task 4 report；没有运行宿主全量测试，没有推送、发布或触发 workflow。

上述为集成阶段历史结果；其旧链退役等待条件已被更新的 0025/0025a 取代。
本地退役后的最终包与当前 Host 验收见下节，双平台 Actions 仍是最终交付义务。


首轮 Actions [38084193094](https://github.com/XLCYun/narrant/actions/runs/38084193094/job/114307126707)
在 macOS 脚本测试阶段发现过时的 workflow guard 断言，尚未完成语言验收。修正测试以实际条件分别验证
lifecycle 独占、聚焦语言跳过旧 build、完整发布依赖/显式开关/main 分支；生产 workflow 不变。
同一六文件 Node 测试组本机 44 passed、0 failed、2 Windows-only skipped；预期负例日志改为捕获并验证。
该修复不代替双平台重跑；当时仍保留旧链，现已依照本地验收政策退役。

## 审查修复：JSX 语义空白（T4-I1）

固定 TypeScript 的 `JsxText.getStart()` 会跳过前导空白，不能用作 JSX 文本的语义起点。
规范化继续使用原生 printer 和叶节点，只对 `JsxText` 使用完整 `pos..end` span。
新增 `<div>a</div>` → `<div> a</div>`、`<div></div>` → `<div> </div>` 两组回归：
投影测试保留不同内容，真实 Native helper/Host 图差异均仅产生 `render` 节点的 `content=Modified`。
20 个投影测试通过；macOS ARM64 本地候选支持包的真实图回归通过。双平台 Actions 验收仍未完成。

## 审查修复：原生早期诊断（T4-I2）

TS 6.0.3 的 `getSyntacticDiagnostics` 对 TS 文件只返回 parse diagnostics。`return`、严格模式 `with`、重复参数等错误分别来自 checker 或 binder；`Diagnostic` 没有“grammar/early error”分类字段。
固定引擎源码 `typescript.js` 的 `plainJSErrors`（127388 起）提供 91 个原生诊断名称，但包含普通类型限制；`checkSourceFileWorker`（91638 起）还会因 `noCheck`、`@ts-nocheck`、checkJs 和项目引用重定向跳过检查。因此不能直接接受全部 semantic diagnostics，也不能只取常见的三个错误码。

`syntax.mjs` 集中列出 99 个原生诊断名称，按严格模式/模块、控制流/异步、绑定模式/赋值、类/访问器、其余原生语法组织；来源是上述 native plainJSErrors 的 binder/grammar 分组和同一 checker 的 early-error 入口。另有两项只接收本文件 bindDiagnostics 的重复绑定诊断（2300/2451），避免把跨文件 script 冲突升级成解析失败。未纳入一般类型、目标版本、重复普通对象属性、私有方法赋值等诊断。旧 Oxc 接受重复默认导出、重复导出声明和导出别名冲突；这是既有宽松行为，迁移保留它，包括排除位于原生 ExportSpecifier 的重复导出 2300。

读取诊断的事务完全同步，复用同一 LanguageService 的 Program、SourceFile 和 checker，不创建第二份语法树。它暂时解除检查抑制，并仅对当前文件解除项目引用检查跳过条件，调用固定引擎的 `checker.getDiagnostics`；finally 按原属性描述符恢复 options、SourceFile.checkJsDirective、Program.isSourceOfProjectReferenceRedirect，区分“不存在”和“值为 undefined”。原始文本、节点身份和 ordinary semantic diagnostics 的缓存/过滤政策保持不变。该事务中没有异步或导航调用。

2339/2304/18013 的私有名诊断会与普通接收者类型错误共码。只有诊断定位到原生 PrivateIdentifier 时，才沿原生 class 祖先查询原生实例/静态类型和私有成员符号：没有词法声明才失败。这里没有自建 scope/binding 索引，也没有通用 AST 语法校验。回归覆盖未声明私有名、错误接收者类型、外层/嵌套、静态及遮蔽。

验证包括原审查的三个反例；各早期错误家族与普通类型/目标错误的拒绝/允许对照；noCheck、checkJs、nocheck/ignore/expect-error 指令的冷/暖缓存；异常时的属性恢复；无 dist 项目引用。代表性测试还与保留的旧 Document::parse 对照，明确发现并保留上述重复导出的宽松政策。既有真实图组继续覆盖 union、JSDoc、跨配置、无 dist 引用和回调导航。

维护成本限定在固定 TS 引擎升级：重新核对这份命名分类、plainJSErrors 与 checker 发出位置，以及 `checker.getDiagnostics`、SourceFile.checkJsDirective、Program.isSourceOfProjectReferenceRedirect、原生私有符号查询这些内部 API；重跑已保留的拒绝/允许、事务和真实图组。它们不是跨版本稳定 API，升级不可只修改版本号或动态猜测能力。当前继续固定 TS 6.0.3；旧链现已在本地验收后退役，双平台最终验收仍待完成。


## 本地 macOS 验收与旧链退役（2026-10-11）

本轮使用 Node 24.18.0 和正常构建入口，不修改用户已安装的包。最终产物为
`typescript-javascript@1.1.0 / aarch64-apple-darwin`，归档大小 45093622 bytes，
SHA-256 `f05c4e02d052d4c3116b84f97604f32c52fc825f262f8fdade14dd1dcc924157`。
255 个清单文件中不存在 TLS、tsserver、typingsInstaller 或 watchGuard 入口；
全部六个 helper 模块与已审查源码逐字节相同。构建时先验固定 npm SHA-512，
再移除独立服务器文件；Compiler/LanguageService 的 `typescript.js`、标准声明、
包元数据与许可证保留。库内原生 server 命名空间属于上游 API，没有修改上游引擎。

退役的目录是 `src-tauri/src/structure_review/typescript_analyzer/`（旧 AST、投影、
项目准备、TLS/LSP 传输与测试协调），以及 `scripts/setup-typescript-analyzer.mjs`
和 `setup:typescript-analyzer`。七个 Oxc 直接依赖全部删除：allocator、ast、ast_visit、
parser、span、codegen、semantic；Cargo 锁文件只删去 39 个不再消费的包，没有新增或升级包。
归档断言移入 `scripts/tests/archive-validation.test.mjs`，共享 validator 继续供包构建使用；
configDir 夹具保留在 `structure_review/tests/config_dir_fixture.rs`。

旧 22 项 graph_service 业务断言完整保留在 `native_languages/tests/typescript_graph.rs`；
旧内容、参数属性、workspace/config 和早期语法断言保留在本记录前面的迁移表对应测试中。
旧 identity/source 直接比较冻结为 23 节点历史契约，仍比较 ID、类别、native_kind、
name、source、contains 与总节点数。`code.calls` 的 external/symbol 修复断言保持原样。
删除的是旧 transport/私有 Oxc 数据结构和旧服务器协调测试，不删除这些业务覆盖。
生产 TS/JS 工厂与 Review 使用公共 Native Host，Rust/Python 生产路由保持原状。

Task 4 时双平台 workflow 保留现有验收矩阵与 Windows ProcessTree/LSP 定义，仅更新归档测试路径和执行列表；
Task 7 在三种语言退役后删除零消费者 LSP 定义，继续运行唯一生产 ProcessTree 与公共 Native Host。
TS 聚焦 guard 同时校验 26 个真实图测试、模块注册及安装/Review/公开 graph 测试入口，
避免退役后留下零测试筛选器。当前包必需文件、版本、control、Node 启动参数已是原生配置，
本轮无须重复改动这些已审查契约。

最终裁剪包和退役后 Host 的聚焦 runner 结果：helper 87/87、TS 图 26/26、
独立安装 1/1、Review DTO 1/1、生产工厂安装 1/1、公开 graph/diff 1/1；
公共 ProcessTree 3 passed（1 通用内部进程夹具 ignored）、公共 Host 26 passed、
原生应用回调 5 passed、工厂/平台 11 passed。所有组实际执行且失败为零；
另门语言真实工具测试维持 ignored，未运行 Rust/Python 语言套件。
脚本组 50 passed、0 failed、2 Windows-only skipped；修正通用 guard 的陈旧 core 前提后，
保留临时缺 binary 清单的编译前拒绝断言，最终相关文件 10/10 通过。
`check --lib`、`clippy --lib --tests -- -D warnings`、fmt check、作用域 ESLint 与 diff check 通过。
精确命令、完整结果、包身份和保留/删除边界记录在本地 Task 4 retirement report。
未运行宿主全量测试（留待 Task 7），未推送、触发 Actions/quality-gate、发布或合并。
本地完成后继续 Rust 迁移；最终集中运行 macOS/Windows Actions 时由控制者提醒恢复额度，
记录执行 URL 与 commit，处理真实平台失败。该记录不声明 Windows 或整个迁移已验收完成。

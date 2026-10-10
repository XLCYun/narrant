# TypeScript / JavaScript 原生 Helper 迁移证据

本记录对应规格 0025 的 Task 4 集成候选，基线 `1fd3fa0`。本机 macOS ARM64 验证不能替代
macOS ARM64 + Windows x64 Actions。候选支持包 `1.1.0` 保持 `published=false`、
`verifiedTargets=[]`；旧 TS 子树和 Oxc 依赖暂留到双平台验收完成，生产入口已切换公共 Native Host。

## 固定工具和边界

- Node 24.18.0、TypeScript 6.0.3、@types/node 22.20.1、undici-types 6.21.0。
- Helper ID `narrant-typescript`，版本 `0.1.0`，projection `typescript-source-v1`，profile `typescript-static-v1`。
- 一侧一次 NDJSON 请求。Compiler AST、LanguageService Program、类型诊断和定义导航在同一进程内；
  不启动 TLS/tsserver/LSP，不加载业务插件、不写项目、不运行用户代码、不下载项目依赖。
- 复用旧受限 workspace/config 文件系统的政策：第一方源码、必要 JSON 元数据、固定声明闭包。
  business node_modules 中的源码/声明不可读；包配置 JSON 可按既有政策提供 extends 元数据。
- 同步 Compiler API 执行期间由公共 Host/ProcessTree 执行强制取消、等待回收；没有额外进程框架。
- 生产支持包工厂与 Review 入口已接通；测试可注入闭包，生产不再装配旧 TS fallback。

## 兼容性判断

身份编码按 Rust `form_urlencoded::byte_serialize`，空格是 `+`，中文是 UTF-8 百分号编码，
标点逐字节编码；没有直接使用 `encodeURIComponent`。身份中的仓库相对路径保留平台原生分隔符，
协议 evidence 路径保持 `/`。真实旧 Oxc 声明与新 helper 对比了类别、native_kind、名字、ID、source、contains；
夹具含 Unicode、空格、标点、TS/JS、重载、参数属性和嵌套声明。Windows 由同一对比测试实际验证。

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
- 其余聚焦 runner：公共 ProcessTree 3 passed（1 Windows-only ignored），原生应用回调5/5、工厂/平台11/11。
- 构建/归档前后真实图 smoke、`check --lib`、Cargo fmt、作用域 ESLint、`git diff --check` 通过；脚本测试36/36。

安装/Review/公共进程整组在别名和全测试短路收尾前已通过；最后改动后只重跑直接受影响的 Node、TS 图和公共 Host。
最终候选包已重建。坏 tsconfig 不阻止空选集或全部词法测试排除；混合选集仍正常验证配置，越界始终先失败。
完整日志和精确本机命令在 ignored Task 4 report；没有运行宿主全量测试，没有推送、发布或触发 workflow。

剩余验收：控制者以集成 commit 运行双平台 Actions，记录 URL/SHA；两平台均通过后再删除旧 TS 链与七个 Oxc 依赖，
并迁走暂时复用的 configDir fixture、固定身份对照。此记录不代表 Task 4 已完成最终验收。


首轮 Actions [38084193094](https://github.com/XLCYun/narrant/actions/runs/38084193094/job/114307126707)
在 macOS 脚本测试阶段发现过时的 workflow guard 断言，尚未完成语言验收。修正测试以实际条件分别验证
lifecycle 独占、聚焦语言跳过旧 build、完整发布依赖/显式开关/main 分支；生产 workflow 不变。
同一六文件 Node 测试组本机 44 passed、0 failed、2 Windows-only skipped；预期负例日志改为捕获并验证。
该修复不代替双平台重跑，旧链继续保留。

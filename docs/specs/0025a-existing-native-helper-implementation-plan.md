# 既有语言 Native Helper 迁移实施计划

状态：执行中，2026-10-11。权威规格为 [0025](0025-migrate-existing-analyzers-to-native-helpers.md)，
架构决定为 [ADR-0035](../adr/0035-migrate-existing-analyzers-to-native-apis.md)。
基线提交：`14bb67c26ca8ad60ce36c47369047d16e34d1179`。

## Global Constraints

1. 三套语言分析直接使用原生语法/语义 API，最终不保留或嵌套 LSP 语言服务器。
2. 保持现有语言范围、节点身份、归属、原文、关系、测试开关、依赖选项及配置行为。
   允许等价的规范化打印样式变化，不能改变内容差异判定或掩盖有语义的空白/换行。
3. Rust 保留仅仓库、标准库、全部依赖三档；前两档离线，全部依赖沿用用户网络策略。
   不运行 build scripts、过程宏或自动安装项目工具链。TS/JS、Python 保持仓库加标准声明范围。
4. 复用现有 `narrant.native-analyzer/1`、公共 Host 与 `child_process::ProcessTree`。
   新输入政策必须显式表达，禁止用任意命令、环境对象或未校验 JSON 绕开边界。
5. 一侧一次 helper 请求，源码与图来自同一固定侧；选集决定成员，其余允许源码只作上下文。
   提取失败、协议错误、取消及不完整加载不能变成成功空图。
6. 每门语言均需 macOS ARM64 和 Windows x64 的真实 helper、公共图、支持包安装及 Review 接入证据。
   每门验收后删除相应旧链路；所有迁移完成前不宣称整项工作完成，不发布未验收支持包。
7. 所有 Cargo 入口使用 `scripts/cargo-agent.sh`。不并行冷 Rust 编译，不共享 divergent worktree 的 target。
   实现阶段运行聚焦测试，最终运行一次宿主 Rust 全量测试；之后只有新修改或已证实回归才扩大重验。
   不手动触发 quality-gate。
8. 方法、结构体、类使用易读中文注释；Rust 私有测试置于模块 `tests/` 目录，通过 `#[path]` 引用。
9. 不新增常驻池、缓存框架、永久双路选择、自动回退或用户设置；删除被替换的依赖与测试协调代码。
10. 每个任务交付独立提交、实际验证命令和结果；控制者安排审查。实施者不再派生子代理或审查者。

## Task 1: 验证并建立 Rust 原生分析核心

目的：在全面迁移前证明现用 rust-analyzer 的关键语义能力能够直接调用，并留下可复用的 helper 核心。

范围：新建 `native-helpers/rust/` 的独立 Rust crate、锁文件、固定构建工具链和聚焦测试。
原生库整体锁定 `9074e9b4c6bc31d7986ccf4e22d18af70e5508da`；不得混用应用当前 parser 与另一 revision 的语义模型。
先检查实际依赖与可构建性；如上游固定源码不能直接集成，报告具体错误和最小修正建议，不偷偷换 LSP 或减少能力。

- 直接建立 `RootDatabase` / `Vfs` / 分析快照并读取同源 AST。
- 对小型多文件 Cargo fixture 查询定义、父模块和出向调用，记录与旧入口的实际语义差异。
- 明确配置三档依赖、sysroot/rust-src、测试 cfg，关闭 build scripts、过程宏和自动安装。
- 验证 crate 根需要的附加处理；不把 Cargo.toml 的导航位置当作源码声明。
- 聚焦验证缺 rust-src 不自动安装、离线档不下载、build.rs/过程宏标记不执行以及 fixture 工作区不被改写。
- 原生库的 degraded load 错误必须可观察，不把部分工作区加载作为成功依据。

本任务不切换应用入口、不删除旧实现、不实现整张图、不添加独立长期诊断工具。
测试和可复用核心留在 helper crate；临时调查文件放计划 scratch，重要结论写入
`docs/research/existing-native-helper-migration-validation.md`，明确本机证据范围。
Cargo 一律经过仓库脚本，若需安装 helper 构建工具链，应与被分析项目的工具链发现分开。

## Task 2: 验证并建立 Python 原生分析核心

依赖 Task 1 的验证方式；与其代码没有语言语义依赖。
范围：新建 `native-helpers/python/` 独立 Rust crate、锁文件、可复用分析核心与聚焦测试。
以 ty `0.0.84` 对应 Ruff revision `11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a` 为起点，
使用 `ProjectDatabase`、同源 `parsed_module` 与 `ty_ide::goto_definition`，不启动 ty server。

- 通过明确 System/项目环境只读加载 fixture 与标准声明，禁用 uv、解释器及 venv 发现和项目命令。
- 验证 Python 3.7–3.14 的本地/标准库定义、`.py/.pyi`、别名/重绑定、property/overload、Unicode。
- 比较当前 Ruff 0.0.15 与该 revision AST/内容投影所需接口，记录版本对齐问题；不得提高 Python 下限。
- 验证污染环境和逃逸链接不会扩充上下文，标准库内嵌文件的来源能够映射到稳定外部符号。
- 保留缺依赖时的歧义，不用原生 API 的导航结果直接冒充确定的运行时调用目标。

本任务不接应用、不做完整图、不复制旧 LSP 协调代码。更新同一迁移验证记录，区分源码事实、
编译通过、真实查询通过和尚待全量语言验收。

## Task 3: 补齐公共契约、打包入口与验收通道

依据 Task 1/2 已验证的目标加载方式，补全七套 helper 共用的接入约定。
主要文件：`native_analyzer/{mod,input,protocol,graph}.rs`、`native_languages/`、
`language_support/{model,tests}`、打包脚本、支持包清单及 `.github/workflows/language-support-packs.yml`。

- 优先用明确 profile 表达 Rust 三档依赖；定义 Rust/TS/Python 必需的标准声明类型与校验。
  允许真正不需要标准声明的 Rust 仅仓库模式，不全局放松当前四套 helper 的平台要求。
- 保存现有四套协议兼容；若探针证明需改变严格消息字段，先在规格中记录实际理由、版本策略和迁移校验。
- Rust helper 若共享协议 DTO，使用与 Tauri 无依赖的小模块/crate，仅承载实际共用契约，避免框架化。
- 应用共用 Native Host 接入，Rust 依赖选项不得在应用闭包/工厂之间丢失；保留空选集和文件级测试过滤行为。
- 打包器增加新 helper 的构建/入口机制，但仅对已完成语言更新实际支持包版本和公开工厂注册。
  不覆盖旧版本、不自动发布，不把只有核心探针的 crate 作为正式支持包。
- 现有四语言写死的验证枚举改为明确的已迁移集合；CI 可按语言运行聚焦双平台验收，仍保留最终全语言门禁。
  不因少装工具跳过真实验收，不另造一套并行进程测试夹具。
- 给旧 LSP 测试和新 helper 测试分别保留正确边界，最后一门迁完后才能删除无消费者的 LSP 公共层。

验证：公共协议/工厂/打包/CI 脚本聚焦测试；现有四套的消息构造与 graph/来源校验回归。
本任务不声称任何旧语言已经完成迁移。

## Task 4: 完整迁移 TypeScript/JavaScript

新增 `native-helpers/typescript/`，使用现用 TypeScript 6.0.3 和受控 Node，统一 AST 与语义来源。
可迁用既有 `runtime/config.mjs`、`workspace.mjs` 的受限配置与文件系统规则；移除 TLS/tsserver 通信依赖。

- 完整实现现有声明分类、身份、source、签名/内容、contains、关系和外部符号；提供协议 coverage/origins。
- 保留 ts/tsx/mts/cts/js/jsx/mjs/cjs、JSX/JSDoc、重载、回调归属、路径过滤、tsconfig/jsconfig、
  extends/references/configDir、paths、package imports/exports、无 dist 项目引用及默认配置政策。
- 直接调用 Compiler/Language Service API；导航行为不能因换成 checker 绑定就静默改变。
- 先迁移 `own_content_excludes_children_but_keeps_callbacks`、`formatting_and_comments_do_not_change_diff`、
  `meaningful_javascript_text_survives_normalization`、`sparse_array_hole_positions_and_counts_remain_distinct`、
  `embedded_declaration_edits_remain_in_complete_expressions`、`full_expressions_ignore_formatting_and_optional_trailing_commas` 的断言。
- 项目/边界保留 `graph_project_conflicts_keep_unknown_calls_and_local_targets`、
  `graph_workspace_references_without_built_dist`、`fixed_service_excludes_business_and_supplies_node_closure`、
  `fixed_service_rejects_external_source_and_configuration_paths` 及 configDir/workspace 测试输入。
- 更新支持包版本、control、必需文件和 Node 启动参数；用真实图 smoke 替换仅 TLS 版本/transpile 检查。
- 在开发分支接通生产工厂与 Review 路径，复用真实安装和混合语言 DTO 测试；完成 macOS/Windows 证据后删除旧 TS 生产子树、TLS 分发及七个 Oxc 宿主依赖。
  通用 `validateArchive` 先迁出旧 setup 脚本再清理，应用构建自身的 TypeScript 依赖保留。

所有影响图语义的差异必须解释并符合 0025，不为保留旧 bug 新增补丁层。已有缺陷与迁移回归分开记录。
双平台验证由控制者运行 Actions 并保存运行链接/commit；未获得证据时任务不得宣称验收完成。

## Task 5: 完整迁移 Rust

基于 Task 1 核心完成统一协议与全部 Rust 图投影，不恢复 LSP。依赖 Task 3 的公共接口。
语法、test_code、symbols 和引用投影规则从旧实现迁入 helper，并改用同源数据库节点/位置。

若 TS/JS 的双平台验收受外部运行资源阻塞，可以先实现本任务独立的 helper 图投影、协议和聚焦验证。
该阶段不启用 Rust 生产工厂、不发布支持包、不删除旧链路；应用接入与正式验收仍按 TS/JS、Rust、Python 的顺序推进。
此阶段完成只代表内部实现就绪，不代表 Task 5 完成。

- 保留稳定身份、词法包含、crate 根、外部声明、宏/闭包、字段类型与既有关系范围。
- 三档依赖、单/多 Cargo 项目、sysroot 冲突、测试过滤、未激活 cfg 声明与加载失败行为保持兼容。
- 复用旧 syntax、test_code、test_scope、toolchain、repository_scope、integration 用例的语言断言；
  LSP 就绪/UTF-16 假服务器测试替换为对应原生加载和查询行为，不保留无业务意义的传输断言。
- 输出完整统一 DTO/evidence，经同一个 Host 构图；更新 Rust 支持包与应用入口。
- 完成真实安装、Review 图/diff、取消、无下载/构建及 macOS/Windows 验收后，删除旧 Rust 分析路径、服务器分发及宿主 `ra_ap_syntax` 依赖。

本任务不能把“核心 API 能查询”当成“完整图兼容”，也不能缩减 all-dependencies 或禁用已有语言语法来通过测试。

## Task 6: 完整迁移 Python

基于 Task 2 核心完成统一协议与全部 Python 图投影，不恢复 ty LSP。依赖 Task 3 的公共接口。

若前序语言的双平台验收仍受外部运行资源阻塞，可先完成本任务独立的 helper、图投影与公共 Host 聚焦验证。
该阶段不启用 Python 生产工厂、不发布支持包、不删除旧链路；实际接入与验收仍按 TS/JS、Rust、Python 的顺序推进。
共享代码只承载 Rust/Python 已有的协议数据与退出处理，不引入分析引擎依赖或另一套进程管理。
阶段审查通过不代表 Task 6 完成，双平台安装、Review 和旧链退役仍为后续必需门禁。

- 迁移对象分类、身份、自身内容、字段、绑定反证、装饰器证明、关系归属与 target 分类。
- 保留 Python 3.7–3.14、`.py/.pyi`、标准库、重绑定、property/overload、跨项目冲突、Unicode/编码、测试过滤和不确定目标规则。
- 迁移 `graph.rs` 与真实 `native.rs` 中语言断言，尤其各 minor、别名链、局部绑定、标准库缺依赖、污染环境、链接逃逸与取消场景。
- 原生 System 拒绝越界上下文和项目执行，不能套默认环境加载；明确标准库虚拟文件来源与外部身份。
- 更新 helper 支持包、工厂、真实安装及 Review 接入；完成双平台验收后删除旧 Python 子树、ty server 分发及宿主 Ruff 依赖。

不得用 Python `ast` 或提高目标版本替代当前 Ruff/ty 能力。无分析结果时明确失败，不自动回退旧服务。

## Task 7: 清理共同旧链路并完成整体交付验证

依赖 Task 4–6 的语言验收。

- 检查并删除零消费者的 `lsp` 生产模块、遗留位置转换、假服务器夹具、下载/setup/smoke 分支和 CI 调用；
  通用取消、source text 或归档功能按实际消费者保留在合适模块，不为删除目录而损坏共享功能。
- 保留 `child_process` 生产 owner 与平台回归；快速 Windows 验证入口直接复用仍在生产使用的代码。
- 全部七套 helper 经过同一公共 Host；生产调用与支持包清单无旧服务器后备；锁文件、许可和打包说明同步。
- 更新 0024/0024a 的适用范围和 0019/0021/0023 的实现描述，保留历史 ADR 的时间背景并链接新决定。
- 运行一次宿主全量 Rust 测试、受影响脚本/前端类型验证以及最终全语言双平台 Actions；
  新修改只重跑覆盖它的验证，只有共享契约变化或真实回归才扩大。
- 审查整分支的规格符合性和代码质量，解决阻断问题；报告迁移证据、已删除复杂度、剩余能力限制和正式 PR 状态。

验收未完成、平台证据缺失或任何语言仍走 LSP，均不能把 goal 标记为 complete。

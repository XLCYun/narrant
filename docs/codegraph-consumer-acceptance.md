# CodeGraph 消费验收映射

以 [规范 0027](specs/0027-independent-codegraph-project.md) 为迁移边界。下表是保留的 38 项
完整 diff 测试；原函数名和差异断言保留，由对应语言的固定入口消费候选。机器可读清单为
[codegraph-consumer-acceptance.json](../resources/codegraph-consumer-acceptance.json)。

入口 `node scripts/codegraph/verify-consumer.mjs <artifacts> --candidates <json> --language <id>`
在 macOS ARM64 或 Windows x64 串行运行选定语言模块；完整 PR CI 使用 `--language all`。
所有 Rust 调用经过 `scripts/cargo-agent.sh`；明确选中但缺包或零测试均失败。
历史函数名中的 `cli` / `helper` 不代表通用入口：当前所有真实调用都绑定
`bin/codegraph-<id>[.exe] analyze`，不传 `--pack` 或 `--pack-root`。

| 语言/模块 | 完整 diff 测试 |
| --- | --- |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_compares_prepared_repositories` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_keeps_closure_references_without_closure_nodes` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_resolves_graph_local_field_types` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_omits_non_field_type_uses_and_data_accesses` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_preserves_crate_roots` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_excludes_test_code_before_graph_analysis` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_all_dependency_profiles_use_real_project_platform` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_syntax_diff_nested_items_in_type_expressions` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_syntax_diff_preserves_literal_contents` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_syntax_diff_preserves_usage_but_ignores_item_order` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_syntax_diff_top_level_macro_arguments` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_syntax_diff_macro_punctuation_jointness` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_syntax_diff_excludes_nested_tests_under_glob_imports` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_syntax_diff_excludes_nested_test_edits` |
| [rust_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_graph.rs) | `rust_helper_whole_graph_project_form_matrix` |
| [rust_syntax.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_syntax.rs) | `preserves_original_source_without_changing_structural_comparison` |
| [rust_syntax.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_syntax.rs) | `nested_body_edits_belong_only_to_their_owner` |
| [rust_syntax.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_syntax.rs) | `closures_are_code_of_their_enclosing_function_or_value` |
| [rust_syntax.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_syntax.rs) | `named_items_inside_closures_keep_their_own_content` |
| [rust_syntax.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_syntax.rs) | `adding_fields_does_not_modify_the_type_own_content` |
| [rust_syntax.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_syntax.rs) | `nested_items_in_type_expressions_have_separate_content` |
| [rust_syntax.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_syntax.rs) | `normalization_preserves_literal_contents` |
| [rust_syntax.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_syntax.rs) | `preserves_child_expression_usage_but_ignores_item_order` |
| [rust_syntax.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_syntax.rs) | `top_level_macro_invocations_have_content` |
| [rust_syntax.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_syntax.rs) | `macro_punctuation_jointness_is_significant` |
| [rust_syntax.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_syntax.rs) | `unicode_and_crlf_keep_precise_source_and_stable_identity` |
| [python_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/python_graph.rs) | `function_body_change_belongs_to_callable` |
| [python_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/python_graph.rs) | `class_method_body_and_local_class_do_not_change_parent_content` |
| [python_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/python_graph.rs) | `conditional_declarations_preserve_execution_order_without_copying_child_content` |
| [python_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/python_graph.rs) | `fields_deduplicate_receiver_bindings_without_stealing_method_content` |
| [python_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/python_graph.rs) | `normalization_and_field_order_distinguish_semantic_content` |
| [python_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/python_graph.rs) | `shared_initializers_and_repeated_fields_are_not_duplicated` |
| [python_native.rs](../src-tauri/src/structure_review/codegraph_client/tests/python_native.rs) | `native_overload_docstrings_participate_in_content_comparison` |
| [typescript_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/typescript_graph.rs) | `graph_test_membership_lazy_context_and_stable_external_symbols` |
| [typescript_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/typescript_graph.rs) | `graph_diff_format_child_parent_and_inline_callback` |
| [typescript_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/typescript_graph.rs) | `graph_dynamic_unknown_names_ignore_format_but_preserve_literal_values` |
| [typescript_graph.rs](../src-tauri/src/structure_review/codegraph_client/tests/typescript_graph.rs) | `typescript_jsx_leading_and_space_only_text_changes_graph_content` |
| [rust_production.rs](../src-tauri/src/structure_review/codegraph_client/tests/rust_production.rs) | `production_profiles_keep_graph_policy_and_readonly_project` |

## 其他保留验收

- `src-tauri/tests/codegraph_graphs.rs` 从旧 `native_helper_graphs.rs` 迁移：七语言真实图、内容差异、
  引用差异、包含关系、源文本、图外符号，以及多语言合并。C# 内部 platform 注入转归 CodeGraph；
  消费者通过真实 `net999.0` 项目声明验证缺目标失败，仍保留 net8/net10 隔离与并集差异。
- `language_support/tests/{service,python,native}.rs` 保留真实归档安装、错误重试、跨进程共享锁、
  删除保护、重启复用、只读分析，并增加旧同版本供应方隔离、公开来源身份与 SHA256SUMS 验证。
  七包在两个目标平台的清单必须包含各自可执行入口和五字段描述符；旧通用入口、错误语言、
  错误平台后缀和不可执行入口均不能通过校验。
- `scripts/tests/codegraph-consumer.test.mjs` 验证下载流中断、发布摘要和解包摘要失败后可原目录
  重试、已验证包复用、旧残片替换和损坏缓存重验；这些合成归档仅验证下载边界。
- `structure_review/application/tests/` 保留 Rust、TS/JS、Python 的真实 Git Review DTO 链路，
  TS/JS 同样经完整生产安装服务；所有语言的工作区保护、并行失败与两侧取消回归继续执行。
- `codegraph_client/tests/transport.rs` 通过读到 stdin EOF 才输出的夹具验证一次性请求、最终
  身份、半行 EOF、空输出、重复键、UTF-8、图不变量和结果/诊断上限；旧 ready/cancelled
  均被拒绝。测试保留拒收封套的 null/确切 ID 和真实失败先于稍后取消的规则。
  两层真实后代矩阵覆盖取消、期限、正常/异常根退出，以及根退出但后代继承管道的情况，
  返回时直接确认后代已退出和 scratch 已清除；Windows 保留准确 HANDLE 后做零超时检查。
- `application/tests/codegraph_lifecycle.rs` 在 macOS 观察真实一次性 TypeScript 入口，不再要求
  不存在的 worker/helper 数量；强制退出以及丢弃 IPC 后 shutdown 均先完成进程、工作区
  与包租约屏障。`codegraph_client/tests/rust_lifecycle.rs` 另观察并暂停真实 Cargo 后代再取消，
  证明调用方回收的是实际 Rust 工具树。Windows 的 Job 原生句柄回归及真实 CLI 取消继续
  执行；Unix PID 观察用例不宣称在 Windows 执行。
- `code_graph/tests/graph.rs` 保留并执行 `merges_language_graphs_and_rejects_identity_or_symbol_collisions`
  和 `mixed_languages_and_all_reference_target_states_are_supported`，这两项消费端合图断言
  不以生产者内部 API 或 38 项差异清单替代。
- Narrant 保留 `child_process` 对 Git 等原调用者的测试，以及 Review 生命周期与工作区回归。
  Unix 递归后代快照、出生时间识别和 zombie EPERM 修复与 CodeGraph 审查版本一致。

## 转归 CodeGraph 的验收

七套 `native-helpers/`、私有 `native_analyzer` 的来源证据/Host 协议、`native_languages`
的 SDK 发现、只读 loader 与运行时工厂，其源码测试归 CodeGraph CI；Narrant 不再重复构建 helper。
旧图模块中不依赖 diff 的纯语义测试亦由 CodeGraph 的
[semantic-acceptance.json](https://github.com/XLCYun/CodeGraph/blob/main/resources/semantic-acceptance.json)
逐项保留。旧研究文档继续作为有日期的迁移证据，不能替代当前发布、平台和消费验收结果。

当前生产目录为 unpublished。本机候选、真实 release 下载、双平台完整 CI 三种证据分别记录；
缺包不能通过 skip 假装验收完成，完整发布后的 CI 是最终合并条件。

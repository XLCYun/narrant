# Direct language-program public protocol refactor

关联 Narrant #205。2026-10-22 Q16 已明确接受完整重构。此计划是既有提取工作的修订，不重跑旧计划已完成任务；历史证据、28项旧rulings和候选保留。当前实现基线仅用于比较，不代表新结构已通过验收。

## Spec

CodeGraph `docs/specs/0001-standalone-language-analyzers.md`、`docs/protocol.md` 与 ADR0001；Narrant `docs/specs/0027-independent-codegraph-project.md` 与 ADR0039。运行确认在 `/Users/lizhanfa/.codex/cangyun-loop/github.com/XLCYun/narrant/issue-205-clarification-08-confirmed-scope.json`，run `bc79dd6d127a4970aa6519e45c9acfa3`。

## Global Constraints

- 用户已确认完整范围；不再询问已决定事项。CodeGraph 主规范为 `/Users/lizhanfa/projects/CodeGraph/docs/specs/0001-standalone-language-analyzers.md`，公共线契约为同仓 `docs/protocol.md`，Narrant 主规范为 `docs/specs/0027-independent-codegraph-project.md`。
- 保留七语言、macOS ARM64/Windows x64、固定 `bin/codegraph-<id>[.exe] analyze`，协议 `codegraph.analyzer/1` 和未发布的 `0.1.0`。每语言程序直接实现公共 JSON、目标 SDK 发现、构图和必要校验；无跨语言 Rust 宿主、私有消息跳转或分析 worker。
- stdin 是一个 UTF-8 JSON 后 EOF，尾换行可选；stdout 仅一个带 `protocol/engine_version/pack_id/type/id/outcome` 的最终 JSON 后退出。删除 ready/cancel/cancelled；调用方取消与超时终止整棵进程树，正常 EOF 不取消。不得恢复监督进程或控制线程。
- 保留显式空集免项目/SDK发现、按配置发现、精确选集、图外引用、六类图与身份/自身内容语义、测试开关、Rust三档依赖、来源与只读和SDK政策；不执行项目构建脚本或安装目标SDK。输入不接受私有catalog、命令、运行时或SDK路径。
- 四个原生入口；C# SDK self-contained apphost 与随包 .NET；TS/Java 薄入口仅定位运行时、固定参数/必要环境、继承标准句柄及退出传播。macOS exec，Windows spawn/wait。无公开 Rust SDK、HTTP、常驻服务、差异接口、新语言或平台。
- 不删弱化现有语言语义/来源/只读断言。旧私有测试改用同进程组件入口或不随包交付的测试驱动；不留生产私有协议兼容模式。移除旧控制消息测试须记录替代的公共或调用方生命周期覆盖。
- Cargo 统一通过对应仓库 `scripts/cargo-agent.sh`，每次仅一个冷构建，不共享 divergent worktree target，不禁用 incremental。冷原生编译可用 `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` 节约磁盘，命令须记录；先 check --lib 与聚焦测试，完整双平台由自动 PR CI 获取；不手动触发 quality-gate。
- 中文注释说明新增方法/类型；Rust私有测试置于模块 tests/，不内嵌生产文件。保持既有格式、锁定依赖、许可证和双平台路径修复。阶段中旧Host集成暂不兼容由Task4统一迁移，不得为绿灯保留旧协议或删除语义用例。
- 复用当前专用分支；生产者起点 ff7e648ecace3aeca672064bc3eb6ca37ffb729e，消费者起点 b594535116654e101658e8d2b94f2719e6c0b082。只允许一个实现者写源码；实现者不派子代理、不推送、不发PR、不合并、不发布。父代理安排独立任务审查与最终审查，保留用户模型设置。
- 人工合并权不变。首次发布须在生产者人工合并和确切 main 完整CI后，按既有授权执行。生产 published 标志在真实发布字节通过消费前保持 false；#205 在两仓交付与收尾前不完成。历史CI/候选不可冒充新结构证据。

## Architecture and interfaces

语言程序把公共请求解析一次，构造本语言内部 AnalysisContext（可复用现有类型，但不能序列化后走私有消息）；直接调用原有语义核心并输出公共图。共享 Rust 代码限 Rust/Python 等同语言实现，不再适配其他语言进程。必要图检查在自然构建/输出边界执行一次。

最终响应新增 `engine_version` 与 `pack_id` 以承接移除 ready 后的身份校验；图形状与其余请求字段保留。单JSON读取至EOF支持无尾换行/合法JSON空白，拒绝第二JSON值。错误无graph，成功exit0，已处理失败exit1，CLI用法exit2；崩溃或强杀可能无JSON。

最终清除 root `src/` 的分析宿主。薄入口放 `launchers/`；根 Cargo 可仅保留 launcher workspace，不公开通用分析库。`analyzers/common/` 是同进程 Rust 基础代码，不能 path-import root src。新包把 JS/Java 代码置于 `runtime/analyzer/`；C# 完整self-contained发布位于bin。既有 `control/codegraph.json` 和平台数据可继续作为包内元数据，非第二个消息协议。

## Task 1: Rust/Python direct protocol and shared native foundations

**Owner:** fresh producer implementer. Repository `/Users/lizhanfa/projects/CodeGraph`.

Read the Global Constraints file supplied by the parent, accepted spec and docs/protocol.md first. Scope: `analyzers/common/`, `analyzers/rust/`, `analyzers/python/`, common protocol fixtures/tests, and prepared producer glossary/spec/ADR/protocol docs. Do not change other languages, packaging, Narrant or root Host implementation except extracting necessary shared utility into common without breaking remaining imports.

1. Add the actual one-shot public protocol, fixed identity, CLI help/version/usage, bounded single JSON decode, selection/root/package boundaries and final output for both native programs. Bind language in code; rename public binary targets to codegraph-rust and codegraph-python. Directly call original Rust/ty cores in-process; no private JSON roundtrip or worker/control thread.
2. Move required Rust SDK/scope setup from old adapters into Rust analyzer. Python prepares its own fixed vendored declarations and approved Python target context. Preserve all discovery, read-only/source and semantic behavior. Explicit [] returns valid empty graph before configuration/SDK lookup.
3. Common owns only reusable Rust DTO/input/graph checks and actually needed source/process utility. Remove common imports reaching root src. Replace one_shot ready/cancel machinery; do not retain production codegraph.engine/1 mode. Internal component contexts can retain catalog/limits types while they stop being a production message format.
4. Preserve Rust/Python semantic tests. Adapt transport tests to public one-shot; private component fixture data may drive internal functions/test-only binaries. Provide shared representative conformance vectors covering identity, normal EOF, no final newline, trailing second JSON, invalid protocol/fields/paths, empty selection, handled error with no graph, and old-private request rejection.
5. Commit all prepared producer domain docs plus Task1 source/tests. Parent-owned Narrant docs are out of scope. Report exact commands, outputs, semantic-test mapping, source HEAD and concerns to task-1-report.md.

**Validation:** common crate tests; Rust/Python check --lib and affected native tests through cargo-agent, with disk-conscious debug profile as permitted; actual new binaries exercised on temporary package fixtures for public empty/invalid input and at least one real nonempty project per language. Preserve raw logs; existing project SDKs/toolchains are available under .tools and standard paths. No all-seven candidate claim yet.

## Task 2: TS/Java direct programs and startup-only launchers

**Owner:** fresh producer implementer after Task1 review.

Scope: `analyzers/typescript/`, `analyzers/java/`, new `launchers/` and minimal root launcher-workspace wiring if needed. Read the finalized public contract and Task1 conformance vectors, not the entire plan. Root Host stays until Task4; packaging path updates outside launchers are deferred there.

1. JS and Java Main implement the public request/result directly, including compiled identity, input/root/selection checks, own SDK/declaration discovery, necessary graph checks, [] shortcut and CLI behavior. Port relevant target policy from old adapters/platforms, preserving standard SDK hints and Windows Unicode/path corrections. Do not parse private envelopes, add control readers or serialize into private helper messages.
2. Launchers bind TS/JS or Java statically. Compute package location from canonical executable, set only fixed runtime argv and necessary runtime environment, inherit stdin/stdout/stderr directly. macOS exec replacement; Windows child spawn/wait and exit propagation. No protocol, SDK or graph logic. JS/Java resources use runtime/analyzer (builders switch in Task4). No SEA/jpackage work.
3. Preserve TypeScript compiler/JDT graph/source/discovery/SDK semantic assertions. Adapt tests via public package fixtures or same-process component APIs; update control-only cases with a mapping. Test launcher unrelated cwd/Unicode paths, help/version forwarding, inherited pipes and abnormal runtime exit.
4. Commit and report in task-2-report.md, including actual Node/JVM tests and bounded-launcher source review. No pushes or publication.

**Validation:** full affected JS/Java native suites using existing pinned Node24/JDK25; launcher formatting/check/tests via cargo-agent and actual macOS exec+pipe/exit smoke with real Node/JVM. Windows runtime evidence belongs to Task4 automatic CI and final gate; record this limitation explicitly.

## Task 3: Go/C#/C++ direct programs and C# apphost

**Owner:** fresh producer implementer after Task2 review.

Scope: `analyzers/go/`, `analyzers/csharp/`, `analyzers/clang/`; C# publish helper if necessary (coordinate exact file ownership with parent). All prior wire and launcher paths are fixed inputs.

1. Three programs implement public JSON and CLI directly with fixed identity, [] shortcut, root/selection boundaries, own SDK/catalog discovery and necessary graph validity. Port Go version selection, .NET reference pack/TFM, macOS SDK and Windows/MSVC headers from current adapters/platforms; preserve all standard-SDK and Unicode/extended-path behavior. Keep existing semantic libraries and fixed versions.
2. Remove ready/cancel/private message production paths and control listeners. Keep real language/library threads and tool queries only as needed. Existing limits/read-only policies remain; no separate Rust validator or automatic graph repair.
3. C# publishes platform self-contained apphost with fixed codegraph-csharp name and .NET10 runtime; generated apphost loads C# Main in-process. Keep all Roslyn/native dependencies and licensing; do not rename dotnet into an analyzer. Prepare reusable publish output logic for Task4 package builder.
4. Preserve native semantic/source/SDK assertions; use component fixtures or public package roots rather than a production private backdoor. Map lifecycle test changes. Commit and write task-3-report.md.

**Validation:** existing Go native tests, C# native tests and actual apphost smoke with public JSON on nonempty fixture, C++ native/source/selection tests against existing LLVM SDK; format/build checks appropriate to changed code. Record macOS coverage and Windows cases waiting for automatic CI, with raw logs and exact pinned tools.

## Task 4: Remove the Host, rebuild distribution and prove producer integration

**Owner:** fresh producer implementer after Task3 review. Sole producer source/build owner.

1. Delete old root src analysis Host/adapters/worker/private protocol, old copies of native engine binaries, and obsolete root crate dependencies. Migrate genuinely used common or test process utilities first, including Windows probe. Root Cargo may be launcher workspace only; no code imports a deleted root module or Narrant. No __worker, private-protocol fallback or root graph conversion remains.
2. Update all package/version/build/verification/CI scripts, examples and current docs for the seven direct programs, runtime/analyzer paths, C# complete apphost publish files, public final identities, and one-shot EOF. Keep immutable manifests, path/link boundaries, limits, hashes, licenses and strict fixed version/source provenance. Preserve package control metadata as needed, not as a private message interface.
3. Adapt public seven-language conformance and language-native launch drivers. Keep semantic assertions and meaningful acceptance mappings; remove obsolete worker-count/cancel-message expectations only with replacement caller-tree or one-shot coverage. Include no-final-newline, stdin EOF, missing/malformed result, actual stdio/exit and fixed identity; ensure no old architecture can accidentally satisfy tests.
4. Build fresh all-seven macOS candidates from a committed clean revised producer HEAD; sourceDirty=false and exact immutable receipt. Do not overwrite old artifacts. Save artifacts under artifacts/direct-protocol-<sha> and publish their candidates.json path for Narrant. Run actual public package acceptance from unrelated cwd with producer-only environment removed.
5. Run producer standard format/script checks and required local native coverage not already proven for unchanged code in Tasks1–3; no unnecessary full reruns. Keep raw evidence and review source-path/import/production-symbol audit. Commit source before final candidate build; generated logs/artifacts stay untracked. Write task-4-report.md with all exact identities, checksums and remaining real-Windows evidence requirements.

**Validation:** package scripts and verifier tests; actual seven public packaged programs on macOS covering existing discovery/selection/empty/graph policies; archive/manifests/descriptor/licenses/provenance checks; root workspace formats/checks. Parent pushes reviewed source to existing feature PR to obtain automatic Windows and macOS full CI. Complete dual-platform CI and new architecture final review remain parent-owned final gates; do not claim them from local success.

## Task 5: Narrant one-shot client and actual revised-package consumption

**Owner:** fresh Narrant implementer after Task4 review. Repository `/Users/lizhanfa/.codex/worktrees/688d/narrant`.

Read AGENTS.md, docs/language-support-packs.md, docs/codegraph-consumer-acceptance.md, accepted spec0027 and finalized CodeGraph wire contract. Scope consumer client/transport/tests, fixtures/scripts/docs and exact development-candidate configuration; no producer source writes.

1. Remove ready/cancel handling and cooperative grace stages. Start fixed absolute analyzer, write one JSON and close stdin, verify final protocol/engine_version/pack_id/id/outcome, parse safe graph and require successful process exit. No runtime assembly or language analysis moves into Narrant. Retain independent structural boundary validation.
2. Cancellation/deadline/transport failure terminates whole owned process tree (Windows pre-resume Job and macOS ownership), drains pipes and waits before releasing package/workspace leases. Preserve nonzero/abnormal/truncated result errors and failure-vs-cancel ordering. Root kill alone is insufficient. No new caller supervisor process.
3. Preserve downloader/hash/atomic install/retry/lease behavior, graph merges, every retained named diff mapping, Review and application identity. Adapt fake entries and real package tests to final identity/one-shot EOF; test hung pipes, cancellation/timeout and descendant exit, normal EOF and error without success graph.
4. Use Task4 fresh sourceDirty=false immutable candidates for actual all-language consumption and relevant mixed Review/diff/TFM/source paths. Production published=false remains until real release. Update domain/protocol-consumer docs and plan; commit prepared Narrant glossary/spec/ADR with changes. Report exact candidate identities and each real acceptance result in task-5-report.md.

**Validation:** fmt, check --lib, affected transport/process/install/lease/Review/diff tests, script tests, TypeScript/Lint/build as required by existing extraction gate; actual seven-language candidate acceptance preserving all named diff cases. Full Rust issue closeout runs once or via exact successful automatic CI, not duplicated locally after same-code CI success. Do not touch personal app data. No UI change expected; if UI is changed AGENTS browser acceptance applies.

## Task5 本地实现记录（2026-10-22）

消费端已改为一次写入 JSON 后关闭 stdin，只接收带固定协议、引擎和语言身份的最终结果；取消和期限直接回收调用方拥有的整棵树。保留原生 Job/进程组、管道线程、scratch、工作区和包租约屏障。旧 ready/cancel/cancelled 的生命周期覆盖由正常 EOF、异常/正常根退出、两层真实后代、继承管道，以及实际 TypeScript 入口和 Rust Cargo 子进程验收承接。

本机使用独立、clean 的 producer `52830b8b7e1294c44b27cc8b882eb9d6bf8467c9` 七包候选，索引 SHA256 为 `5efba82ca1abbbe9b26ab5c4c6f064a8a656fde3e2e5666efc036f4527782412`，receipt SHA256 为 `14cea79d8c2dfc715ae11b94e19fe56361bb28eb486ef23fca06d32dd6e39a3a`。实际消费22组命令共152次测试通过，38个保留差异函数和六个源文件与消费者基线逐字节相同，并逐项执行通过；两项消费端合图、Review、安装租约、目标 SDK 与 TFM 路径均执行。自查修复错误上下文后，传输8项、Rust全量599项，以及最终源码的七包安装和11项图验收通过。脚本173项、前端452项、类型、Lint、格式和构建通过；原始输出保存在 Task5 报告与日志。

这仅是 macOS 本地消费记录。producer 自动 CI `39768838377` 的 runHead 为528，实际 checkout 是 synthetic merge `99c2350afea3a5606209be7bc5373eb3cfa8608b`；Mac成功，Windows在 .NET runtime archive inspection 失败，Complete CI 失败。不得与本机528清洁来源候选混写，也不得当作已发布或双平台完成。后续修复、任务/最终审查、人工合并、真实发布和消费者发布切换仍按下述门禁继续；生产 published 保持 false，#205 不在本任务完成。

## Review Focus

Review actual ownership after deleting root src, common imports and production protocol symbols, language-local SDK/source checks, Windows extended paths/Unicode, one-shot EOF and identity, malformed/oversize/failure ordering, apphost runtime search and publish contents, launcher stdio/exit and trusted fixed argv, Cargo genuine descendants, lease cleanup, all old semantic assertions and immutable artifact/source evidence. No test may merely check implementation text while missing actual runtime behavior.

## Parent delivery gates and continuation

After each task the parent reads the whole report, obtains both spec and quality task review, resolves load-bearing findings and records completion. Never parallelize source implementers. Keep source heads, test logs and review packages separate from historical extraction evidence.

After Tasks1–5, final review covers the entire new amendment from ff7e648 and b594535, plus load-bearing unchanged language code and preserved historical observations. Parent checks exact new producer PR head automatic dual-platform CI and Complete CI, updates the existing CodeGraph PR with final behavior and evidence, then presents it for established human merge. No auto-merge.

After human CodeGraph merge: require exact successful main CI and immutable release artifacts; execute already authorized first v0.1.0 prerelease; verify all downloaded assets and identities; pin Narrant to those real released bytes, run actual release consumption, get focused cutover review/required automatic CI, create/attach/register the Narrant PR for human merge. These small cutover edits receive their own concrete verification rather than being silently appended to a previous reviewed source head.

After Narrant human merge and all required closeout: preserve exhaustive rulings/deferred observations from both plans in durable runtime, stop tools/builds, clean only authorized generated worktree cache, complete #205/loop task and use already-authorized final controller notification. Do not delete either plan workspace before required evidence/rulings retention and true completion.

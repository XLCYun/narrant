# Task 1 report

Status: DONE_WITH_CONCERNS

## 范围与结果

在 `/Users/lizhanfa/.codex/worktrees/migrate-existing-native-helpers/narrant`、
`codex/migrate-existing-native-helpers` 实现独立 Rust helper 核心。未修改 primary checkout。
精确上游 revision 为 `9074e9b4c6bc31d7986ccf4e22d18af70e5508da`；直接链接原生
`project-model`、`load-cargo`、`ide`、`syntax`、`vfs`。helper 构建版本固定 Rust 1.98.0，
本机 fixture 的实际项目工具链为已有 stable 1.97.1 和已有 rust-src。

直接建立 `RootDatabase` / `Vfs` / `AnalysisHost` / 固定分析快照。可复用接口提供同源 AST、
原文、定义、出向调用和父模块；crate 根通过原生身份与 root_file 补齐 LSP handler 附加
逻辑，Cargo.toml 不作为源码声明。未知文件身份、越界与 UTF-8 字符内部偏移返回错误。

明确三档依赖、网络政策、已有 sysroot/rust-src 与 test cfg。所有档禁用 build scripts、
过程宏服务和自动安装；前两档强制离线。原生 metadata 的 no-deps 回退、sysroot stitched
降级、目标/cfg/工具链和 Cargo target 根文件不完整均转为可观察错误。

实际证明多文件 Cargo fixture 能解析跨文件定义、共享父模块、方法/闭包调用、测试 cfg、
常规/集成测试/自定义空根；不执行 build.rs/过程宏，不下载离线依赖，不改写 fixture。
没有切换应用入口，没有完整图或 native-analyzer 协议接入，没有删除旧入口，也没有发布包。

## 实际发现与修正

1. 新解析锁文件时选择了 unicode-ident 1.0.26，固定 ra-ap-rustc_lexer 0.166.0 编译失败 E0080：
   `unicode-properties and unicode-ident must use the same Unicode version`。
   修正：从精确上游 revision 自带 Cargo.lock 保留传递依赖基线，再解析 helper git 包，
   最终 unicode-ident 1.0.24 / unicode-properties 0.1.4 / salsa 0.28.2。没有修改上游源码。
2. 初次全部依赖 fixture 没有 Cargo.lock；上游 make_lockfile_copy 返回 None，metadata 在
   fixture 根生成锁文件。不改写断言确实失败，未用预创建 fixture 锁文件掩盖。
   修正：native no-deps metadata 取得真实 workspace 根和项目 Cargo 版本，复制已有锁
   约束或在独立 TempDir 创建缺失锁。版本分支沿用上游已使用的锁路径机制。为避免项目
   resolver 锁路径污染 sysroot metadata，完整 sysroot 先由同配置原生模型加载，随后只加载
   全部项目 Cargo metadata 并复用该 sysroot；最终数据库仍一次加载同一固定项目。
3. 核对原生 API 时修正了类型接入：上游 AbsPathBuf 必须明确 UTF-8 检查后转换，file_text
   是 triomphe Arc，sysroot 包数量通过公开 API 检查。所有最终 check/Clippy 无警告。
4. 初始 fixture 断言假定宏展开调用出现在 outgoingCalls，但旧固定服务器与原生 API 实际
   都不返回仅 `invoke!()` 体内的 helper。修正测试预期以保留实际行为；未自行添加语义。
5. 原生 parent_module 对自定义空根返回该源码 `0..0` 的 CrateRoot 导航；旧服务器 handler
   返回 Cargo.toml。核心显式返回 CrateRoot 身份，不把任一导航当作普通模块声明。

## 验证命令与结果

除 rustup 安装与只读 Rust 工具/服务器探针外，所有 Cargo 命令均经仓库 wrapper，未并行
启动重 Cargo 构建。使用独立 helper target 与 incremental，未清理缓存或 target，未手动
触发 quality-gate，未执行宿主 Rust 全量测试。

工作目录均为上述 worktree。Cargo 命令前缀精确为：

```sh
PATH=/Users/lizhanfa/.nvm/versions/node/v24.18.0/bin:$PATH scripts/cargo-agent.sh
```

| 实际命令（在此前缀后） | 结果 |
|---|---|
| `+1.98.0 check --manifest-path native-helpers/rust/Cargo.toml --lib`，初次新锁 | exit 101，E0080 Unicode 版本不一致 |
| 同命令，复制上游锁后 | exit 0，原生依赖成功编译，34.69 秒 |
| `+1.98.0 check --manifest-path native-helpers/rust/Cargo.toml --lib --locked`，核心最终版本 | exit 0，无警告；scratch/task-1-check-final.log |
| `+1.98.0 test --manifest-path native-helpers/rust/Cargo.toml --lib --locked -- --test-threads=1 --nocapture`，初始 10 tests | 5 通过、5 失败；scratch/task-1-tests.log，发现真实锁写回及上述错误预期 |
| 同测试命令，中间修正 | scratch/task-1-core-tests.log，仍保留无锁写回失败的调查记录 |
| 同测试命令，最终 13 tests | 13 通过、0 失败、0 ignored；73.03 秒；scratch/task-1-tests-final.log |
| `+1.98.0 fmt --manifest-path native-helpers/rust/Cargo.toml` | exit 0；scratch/task-1-fmt.log |
| `+1.98.0 clippy --manifest-path native-helpers/rust/Cargo.toml --all-targets --locked -- -D warnings` | exit 0；scratch/task-1-clippy.log |
| `+1.98.0 test --manifest-path native-helpers/rust/Cargo.toml --lib --locked tests::embedded_queries_preserve_multifile_semantics_and_workspace -- --exact`，删除调查输出后 | 1 通过、12 filtered；scratch/task-1-test-cleanup.log |

工具准备命令：`rustup toolchain install 1.98.0 --profile minimal` 成功，版本为
`rustc 1.98.0 (88d9e12ae 2026-08-18)`；`rustup component add rustfmt clippy --toolchain 1.98.0`
成功。默认 Rust 仍为 stable，未安装项目组件。安装器另执行了自己的 rustup 自更新检查。

聚焦测试使用本地独占 HTTP registry trap：仅仓库、标准库、全部依赖 Offline 的请求数
断言为 0；全部依赖 Allow 断言实际有请求，失败仍不能变成成功。文件快照包含目录和文件
字节，检测包括原本不存在的 Cargo.lock、target 目录以及 build/proc-macro marker。
成员 manifest 的真实无锁加载通过；已有根 Cargo.lock 字节复制测试通过。

## 与固定旧入口的实际对照

主证据使用已安装包：
`/Users/lizhanfa/Library/Application Support/com.cyunlab.narrant/language-support-packs/installed/rust/1.0.0/aarch64-apple-darwin/bin/rust-analyzer`。
`resources/language-support-packs.json` 固定 release 2026-09-07 / sourceCommit 9074e9b。
实际版本：`rust-analyzer 0.3.3041-standalone (9074e9b4c6 2026-09-06)`。
二进制 SHA256 `a652ad07e0e6dd42e2eb9910a060e48005cb1e5e9c02786ed6a45b074a5a4c0a`
与 installed.json 的 payload hash 一致。本机复用了已安装包，没有下载、替换或发布包。

准确主对照命令：

```sh
NARRANT_OLD_RUST_ANALYZER='/Users/lizhanfa/Library/Application Support/com.cyunlab.narrant/language-support-packs/installed/rust/1.0.0/aarch64-apple-darwin/bin/rust-analyzer' python3 .superpowers/sdd/0025a-existing-native-helper-implementation-plan/scratch/legacy-probe.py > .superpowers/sdd/0025a-existing-native-helper-implementation-plan/scratch/legacy-probe-pinned.json
```

临时 probe 复现旧 RustLsp 的仅仓库配置、document open、健康等待、definition、parentModule、
prepareCallHierarchy、outgoingCalls 流程。两个 fixture 的状态均为 health=ok/quiescent=true。
结果：helper 定义为 store.rs；store 有 storage/mirror 两个父声明；常规根、测试根、自定义
空根返回 Cargo.toml 导航；update/run/direct_closure 调用 helper；function_value/macro_call
为 []；fixture 文件没有改写。原生 assertions 验证相同源码目标及附加 crate 根身份。

另外先运行了系统 stable RA 1.97.1（8bab26f4）的相同 probe 作为辅助事实，输出保存为
scratch/legacy-probe-stable.json。这不是固定旧入口主证据。临时 probe 脚本和原始日志留在
计划 scratch，不进入正式实现，也未添加长期诊断工具。

## 自审、语义差异与未验收风险

自审确认：生产代码只使用固定原生 API；没有 LSP 依赖/服务器嵌套；无公开命令或任意环境
注入政策；锁路径和网络/自动安装配置在核心内部固定；所选 AST 与语义同源。Cargo 项目根
来自实际 workspace，而非传入 manifest 的父目录。成功时临时 eprintln 调查输出已删除。
代码与测试均有中文注释，私有测试放在 tests/ 并由 #[path] 引入。

原生 API 位置是 UTF-8 字节，旧 LSP 是 UTF-16 行列。parent module 的 Cargo 根附加逻辑
已补齐为身份枚举。其他证明仅覆盖小型 fixture 和查询层，未宣称全图语义完全等价。

Concerns：

- 本机只有 macOS ARM64 与实际项目 Rust/Cargo 1.97.1 运行证据。Windows x64、支持包、Review、
  完整 graph 回归与官方源码其他语义均待后续任务。核心暂只接收单个 Cargo workspace，
  rust-project.json 与多独立项目发现未在此阶段实现。
- Cargo 1.82 以下完整依赖加载使用 --locked：已有有效锁仍可用；需要生成/更新锁的具体
  场景明确拒绝写回并提示用户先准备锁文件。这不等于不支持整个旧工具链。1.82..1.96 的
  锁路径分支目前也只有上游源码核对，真实兼容验收仍需核对参数和平台行为。
- sysroot 内部依赖需要符合用户网络策略和现有缓存；缺组件/metadata 降级均显式失败，
  没有用部分 workspace、自动下载安装或成功空图绕过。
- 全图阶段仍需保留语法层兜底、测试代码筛选、宏/闭包边界、原文与规范化打印、图外目标
  等现有行为，不能把本任务的几个 query APIs 直接当成完成的 CodeGraph adapter。

## 文件清单

- native-helpers/rust/Cargo.toml、Cargo.lock、rust-toolchain.toml、.gitignore、README.md
- native-helpers/rust/src/lib.rs、src/loading.rs
- native-helpers/rust/src/tests/core.rs、loading.rs、support.rs
- native-helpers/rust/src/tests/fixtures/core/Cargo.toml、build.rs、src/{lib,store,specs}.rs、tests/smoke.rs
- native-helpers/rust/src/tests/fixtures/core/macros/Cargo.toml、src/lib.rs
- docs/research/existing-native-helper-migration-validation.md
- 此 Task 1 report（作为本次提交证据）

## 独立审查修复第 1 轮（2026-10-11）

FIX_BASE：`b04e52f992fcc7cec0b0bd45f693cfc7547e773b`。审查指出，原检查只读取
Cargo target 根；上游 loader 对无效 UTF-8 和文件读取错误会静默忽略，因此 `mod bad;`
的非根模块可被漏载而仍返回成功。用现有 fixture/harness 新增
`loading::tests::invalid_utf8_in_declared_module_is_observable`，确实观察到 RED：
`RustProject::load` 成功，测试因“非根模块源码解码失败不能变成部分加载成功”失败。

修复在 `load_cargo::load_workspace` 返回后、数据库被接受之前加入 `ensure_sources_complete`：

- 只读校验原生 VFS 已枚举 `.rs` 的可读性和 UTF-8 解码。原生 loader 对这些文件没有
  初始化 FileText 时，直接求模块图会 panic，所以检查必须先于原生 DefMap 查询。
- 使用同一固定 revision 的 `base-db::all_crates`、`hir-def::crate_def_map` 与
  `DefDiagnosticKind::UnresolvedModule.candidates` 定位活动声明的漏载文件；候选路径
  沿用原生包含文件锚点和 VFS 路径规范化，不自行重建 cfg、`#[path]` 或嵌套目录规则。
- 原本不存在的候选（真实 NotFound）保留为项目诊断；实际读取/解码失败返回有路径的错误，
  磁盘已有且可读取、却未进入 VFS 的活动候选也返回加载错误。
- 没有新增文件遍历、复制/修改 loader 或 catch_unwind。两个新增直接依赖已在原传递图
  中，revision 不变；Cargo.lock 只增加本 helper 的这两个直接依赖名，没有更换锁版本。

新增实际读取失败回归以目录型 `src/bad.rs` 触发，避免 chmod 在 root/跨平台权限下失真。
第三个回归验证缺失模块、cfg 排除候选、`.git` 和 target 排除目录不扩大加载范围；每个
用例仍断言 fixture 目录与字节不变。原有加载后磁盘变化不影响查询快照的测试继续保留。

首轮 16 个测试中，两个错误回归均通过，但范围用例预建 target 暴露 Cargo 新增
`target/.rustc_info.json`（15 通过、1 失败，27.07 秒，scratch/task-1-fix-1-tests.log）。
先换用 `.git` 的中间验证 16/16 通过（26.91 秒，scratch/task-1-fix-1-tests-final.log），
控制者要求保留这个具体只读缺口并修复。按
[Cargo 官方环境变量说明](https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-reads)，
仅在分析配置 extra_env 设置 `CARGO_CACHE_RUSTC_INFO=0`，恢复已有 target 的目录字节断言。
没有修改开发 Cargo wrapper、全局环境、incremental 或 sccache。

API 限制与前提：固定 loader 已在返回前消费 `Vfs::take_changes()` 的原始 Vec<u8>，
返回 VFS 的内容 hash 和 DB FileText map 均没有公开原始字节/无 panic 的缺输入查询。
因此使用只读重读，不用新内容更新数据库或替换原文。调用者须在加载期间固定输入；
本检查不保证并发改写的原子性。该前提已写入核心 load 注释、README 和迁移验证文档。
后续查询仍使用原始快照。本轮没有改全图语义，也没有新增双平台或旧 Cargo 验收声明。

本轮实际命令均在同一 worktree 仓库根执行，Cargo 前缀均为：
`PATH=/Users/lizhanfa/.nvm/versions/node/v24.18.0/bin:$PATH scripts/cargo-agent.sh`。
输出均重定向到 `.superpowers/sdd/0025a-existing-native-helper-implementation-plan/scratch/`。

| 参数/命令 | 结果与日志 |
|---|---|
| `+1.98.0 test --manifest-path native-helpers/rust/Cargo.toml --lib --locked loading::tests::invalid_utf8_in_declared_module_is_observable -- --exact` | RED，exit 101，1 失败、13 filtered；task-1-fix-1-red.log |
| 同上但去掉 `--locked`，解析新增直接依赖 | GREEN，exit 0，1 通过、15 filtered；task-1-fix-1-green.log；锁版本未变 |
| `+1.98.0 test --manifest-path native-helpers/rust/Cargo.toml --lib --locked` 首轮 | 15 通过、1 失败；task-1-fix-1-tests.log；实际 target 写缓存已据此修复 |
| 同上，中间 `.git` 范围用例 | exit 0，16 通过；task-1-fix-1-tests-final.log |
| 同上，恢复已有 target 并关闭分析版本缓存后的最终完整 helper 验证 | exit 0，16 通过、0 失败、0 ignored，26.80 秒；task-1-fix-1-tests-with-target-final.log |
| `+1.98.0 clippy --manifest-path native-helpers/rust/Cargo.toml --all-targets --locked -- -D warnings` | exit 0，1.60 秒；task-1-fix-1-clippy.log |
| `+1.98.0 fmt --manifest-path native-helpers/rust/Cargo.toml` | exit 0；task-1-fix-1-fmt.log |
| `+1.98.0 fmt --manifest-path native-helpers/rust/Cargo.toml -- --check` | exit 0；task-1-fix-1-fmt-check.log |
| `git diff --check` | exit 0 |

改动文件：native-helpers/rust/{Cargo.toml,Cargo.lock,README.md}、src/{lib.rs,loading.rs}、
src/tests/loading.rs、docs/research/existing-native-helper-migration-validation.md 与此 report。
自审确认修复使用原生解析结果，没有新遍历框架/上游改码/任意环境注入入口；相较旧行为，
已加载源码解码失败和活动候选读取失败从部分成功变为显式错误，真正缺失模块仍不阻止
保守分析。新发现的 target 缓存写回随本轮修复，其他兼容性 concerns 仍如上文。
最终自审没有发现本轮新增的未处理正确性问题。未运行宿主全量套件、未触发 quality-gate，
未 push；本轮没有重跑旧 LSP probe，因为未改查询语义或原生 revision。

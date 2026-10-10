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

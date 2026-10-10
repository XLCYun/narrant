# Existing native helper migration validation

## Task 1：直接内嵌 Rust 核心（2026-10-11）

本轮只建立 `native-helpers/rust/` 的独立库和聚焦证明。没有切换应用入口，没有生成
完整 CodeGraph，没有删除旧 Rust LSP 链路，也没有发布支持包。证据来自 macOS ARM64；
Windows x64、支持包安装、Review 接入和全图兼容性仍未验收。

### 固定源码与构建

直接依赖 `ide`、`load-cargo`、`project-model`、`syntax`、`vfs`、`base-db`、`hir-def`，全部来自
rust-analyzer [`9074e9b4c6bc31d7986ccf4e22d18af70e5508da`](https://github.com/rust-lang/rust-analyzer/tree/9074e9b4c6bc31d7986ccf4e22d18af70e5508da)
（上游提交日期 2026-09-06）。AST 与语义数据库没有混用应用现存 parser 的 revision。
helper 固定 Rust 1.98.0；本机被分析 fixture 使用已有 stable Rust 1.97.1 和已有 rust-src。
安装 helper 构建版本没有改变默认工具链，也没有安装被分析项目的 rust-src。

第一次新建 Cargo.lock 时，解析到 `unicode-ident 1.0.26`，与固定的
`ra-ap-rustc_lexer 0.166.0` / `unicode-properties 0.1.4` 不兼容，触发编译期 E0080：
“unicode-properties and unicode-ident must use the same Unicode version”。
最小修正是以该 revision 自带 Cargo.lock 为传递依赖基线，再解析本 helper 的 git 包。
最终保留上游配套 `unicode-ident 1.0.24`、`salsa 0.28.2` 等锁版本。未修改上游语义代码，
未换 revision，未退回 LSP。

### 核心与政策实测

`ProjectWorkspace` 加载后，直接由 `load-cargo` 得到 `RootDatabase` / `Vfs`，再用
`AnalysisHost::with_database` 建立不可变分析快照。同一快照提供 AST、原文、定义、父模块和
出向调用，位置为 UTF-8 字节偏移，导航保留原生完整范围、焦点范围与所有调用位置。

三档配置为：仅仓库 `no_deps=true` 且无 sysroot；标准库仅项目 metadata 传 `--no-deps`，
标准库内部依赖仍加载；全部依赖按显式网络政策解析。前两档强制离线，全部依赖的
`Offline` 也强制离线。sysroot / rust-src 使用明确已有路径，禁止发现分支自动安装组件；
`load_out_dirs_from_check=false`、`ProcMacroServerChoice::None`，并固定
`RUSTUP_AUTO_INSTALL=0`。默认 `test_cfg=true` 对应旧入口上下文；图中测试对象的过滤尚属
后续图适配工作，不能简单用关闭 cfg 代替。

13 个聚焦测试全部通过（0 失败、0 ignored，73.03 秒）：

| 验证 | 本机结果 |
|---|---|
| 多文件 `helper` 定义 | `src/store.rs::helper`，同源 AST 与数据库原文一致 |
| 共享模块源码 | `storage` 与 `mirror` 两个父声明均可观察 |
| 方法和闭包出向调用 | `update`、`run`、`direct_closure` 均调用 `helper` |
| 函数值与宏边界 | `function_value` 与仅有 `invoke!()` 的 `macro_call` 没有出向调用，与旧协议查询一致 |
| 常规、集成测试、自定义空入口 | 均通过 crate 身份识别根，不从文件名或 Cargo.toml 位置猜测 |
| test cfg | 开启时加载 `specifications` 父模块与 `checks -> run`，关闭后没有该语义上下文 |
| 快照与非法位置 | 加载后磁盘修改不改变已加载原文；未知 FileId、越界和 UTF-8 字符内部位置返回错误 |
| 离线下载 | 唯一的本地 HTTP registry trap 请求数为 0；标准库档不会因项目的未缓存第三方依赖下载或失败 |
| 允许联网 | 全部依赖 `Allow` 实际触达 trap；registry 错误仍返回失败 |
| 缺组件/项目工具链 | 缺 rust-src 在原生加载前明确拒绝；缺失项目版本返回加载错误，不使用 helper 版本替代 |
| 项目/sysroot 降级 | 项目 no-deps 回退、损坏 sysroot metadata 的 stitched 回退都不能作为成功 |
| 禁止执行用户代码 | build.rs 和本地 proc-macro 执行标记均不存在，过程宏 crate 本身仍能导航 |
| 不改写 | fixture 目录与所有文件字节均保持一致，包括原本无 Cargo.lock 的全部依赖工作区、成员 manifest 入口 |
| 已有锁约束 | 从真实 workspace 根复制 Cargo.lock 字节；不误用成员目录的锁文件 |

### 上游加载附加处理

不能直接将 `load_workspace_at(...)=Ok` 作为完整加载：
[`ProjectWorkspaceKind::Cargo.error`](https://github.com/rust-lang/rust-analyzer/blob/9074e9b4c6bc31d7986ccf4e22d18af70e5508da/crates/project-model/src/workspace.rs)
可能携带完整 metadata 失败后的 no-deps 回退；sysroot 也可能回退为 stitched 模型。
核心检查项目与 sysroot metadata 错误、原生进度中的 metadata 失败、目标数据、目标 cfg、
工具链信息及 Cargo target 根文件是否可读取。聚焦测试保留两种实际降级失败证据。

独立审查后补齐非根源码检查：上游 `load-cargo` 静默丢弃无效 UTF-8，`vfs-notify`
也会忽略遍历/读取失败。核心用同一 revision 的 `crate_def_map` 与
`UnresolvedModule.candidates` 定位活动声明的漏载源码；Block DefMap 通过原生
`ExpressionStore::blocks` 递归取得，涵盖函数/关联项/常量的 body、签名和字段中的块，
并继续检查块内声明和宏展开。没有运行类型推断或复制 cfg、嵌套目录、`#[path]` 规则。
真正 `NotFound` 的候选仍属于项目诊断；实际读取/解码失败、磁盘已有模块源码却未进入
VFS 均返回加载错误。测试覆盖 root/block 模块读取失败、函数/方法/闭包/常量/签名/
宏展开作用域，以及缺失模块、cfg 排除候选和 `.git` / `target` 排除目录不扩大范围。

该范围测试预建 target 时还发现 Cargo 版本探测会写入 `target/.rustc_info.json`。只对
分析子进程设置 [`CARGO_CACHE_RUSTC_INFO=0`](https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-reads)
后，已有 target 的目录和字节也保持不变；没有更改开发编译 wrapper、incremental 或 sccache。
修复后完整 helper 聚焦测试为 16 通过、0 失败、0 ignored（26.80 秒）。

固定 API 的边界是：loader 返回前已消费 `Vfs::take_changes()` 中的原始字节，返回 VFS
只保留内容 hash，数据库缺输入时的 `file_text` 会 panic，均无公开无 panic 的原始字节
查询入口。Block DefMap 也在返回模块身份之前解析引用文件，无法先用其身份预查字节。
因此整次原生加载/诊断事务外设置一次 `catch_unwind` 异常隔离：保留实际 panic 诊断，
结束本次加载并丢弃数据库。不会逐项用 panic 探测输入，不解析 panic 文本决定语义，也
不继续查询、重试或返回部分结果；原生 Salsa Cancelled 类型继续保留在错误链中，真实
取消/协议分类仍待 Task 5 验收。

源码身份由原生实际解析决定，已经移除第 1 轮按 `.rs` 扩展名前置解码的假设。同一路径
的非法 UTF-8 `.md` 被用作块内模块时返回明确错误，用作 `include_bytes!` 二进制资源时
正常加载；二进制 `.rs` 和 `.bin` 资源也通过。异常隔离没有替换 AST/原文、删掉能力或
修改上游源码。根和活动候选的 I/O 校验只读重读路径；调用者必须在加载期间固定输入，
这不是对并发改写的原子保证。加载后原快照不受磁盘变化影响的测试继续保留。

本机第 1 轮 16 个测试为 26.80 秒；新增原生 block/body 检查后、中间 17 个测试为
52.30 秒；最终 19 个测试（含异常隔离与二进制资源对照）为 52.06 秒、0 失败、0 ignored。
这包含测试与缓存差异，尚不是性能基准；标准库 body 遍历开销需要在后续
性能验收中继续量化，不能从这两个耗时推导生产性能结论。

第 2 轮最终验证命令（仓库根目录，均 exit 0）：

```sh
PATH=/Users/lizhanfa/.nvm/versions/node/v24.18.0/bin:$PATH scripts/cargo-agent.sh +1.98.0 test --manifest-path native-helpers/rust/Cargo.toml --lib --locked -- --nocapture
PATH=/Users/lizhanfa/.nvm/versions/node/v24.18.0/bin:$PATH scripts/cargo-agent.sh +1.98.0 clippy --manifest-path native-helpers/rust/Cargo.toml --all-targets --locked -- -D warnings
PATH=/Users/lizhanfa/.nvm/versions/node/v24.18.0/bin:$PATH scripts/cargo-agent.sh +1.98.0 fmt --manifest-path native-helpers/rust/Cargo.toml -- --check
```

测试日志保留实际缺 FileText panic 输出，测试验证调用者取得明确错误而非部分数据库。
没有为此修改 panic hook，也没有运行宿主全量套件或触发 quality-gate。

首次全部依赖实测发现：上游
[`make_lockfile_copy`](https://github.com/rust-lang/rust-analyzer/blob/9074e9b4c6bc31d7986ccf4e22d18af70e5508da/crates/project-model/src/cargo_config_file.rs)
在原始 Cargo.lock 不存在时放弃复制，metadata 会在项目内生成锁文件。核心先用项目
`--no-deps` 只读加载确定真实 workspace 根和实际 Cargo 版本，再把项目锁文件解析移到
独立 TempDir。已有锁保留约束，缺失锁只在 TempDir 生成。项目的 resolver 锁路径不会
传给 sysroot metadata；完整 sysroot 来自同一配置的原生加载模型。

锁路径策略沿用该 revision 的 Cargo 版本机制：1.97 起使用
`CARGO_RESOLVER_LOCKFILE_PATH`；1.95/1.96 加 `-Zlockfile-path`；1.82 至 1.94 使用
`--lockfile-path` 与上游已有 unstable-options 政策。1.82 以下保留 `--locked`：已有有效
锁可以解析，需要生成或更新锁的具体场景返回明确只读错误。这不是对整个旧 Rust 工具链
的否定；除本机 1.97.1 外的分支只有源码核对证据，需在后续兼容验收中真实运行。

### 与旧 RustLsp 查询的实际对照

主对照复用本机已安装的 Rust 支持包 `rust/1.0.0/aarch64-apple-darwin/bin/rust-analyzer`，
对应 `resources/language-support-packs.json` 的 release `2026-09-07` 与同一 sourceCommit。
版本输出为 `rust-analyzer 0.3.3041-standalone (9074e9b4c6 2026-09-06)`，实际二进制
SHA256 `a652ad07e0e6dd42e2eb9910a060e48005cb1e5e9c02786ed6a45b074a5a4c0a` 与
`installed.json` 的 payload hash 一致；未重新下载或更改支持包。临时 Python probe 复现
`src-tauri/src/structure_review/rust_analyzer/lsp.rs` 的仅仓库初始化、打开文档、等待
`experimental/serverStatus` 健康就绪、定义、parentModule、prepareCallHierarchy 与
outgoingCalls 流程。实际状态为 `health=ok`、`quiescent=true`。

| 查询 | 旧入口协议结果 | 固定原生核心结果 |
|---|---|---|
| `helper` 定义 | `fixture:/src/store.rs` | 同一个源码目标 |
| store 父模块 | lib.rs 中 storage 和 mirror 的声明 | 两个原生导航目标 |
| lib.rs / smoke.rs 父模块 | Cargo.toml 导航位置 | `ModuleParent::CrateRoot` |
| 自定义空 entry.rs 父模块 | Cargo.toml 导航位置 | 显式 crate 根身份；原始 parent_module 返回源码自身 `0..0` 的 CrateRoot 导航 |
| update / run / direct_closure | helper | helper |
| function_value / macro_call | 空 outgoing 列表 | 空 outgoing 列表 |

crate 根的差异来自
[`handle_parent_module`](https://github.com/rust-lang/rust-analyzer/blob/9074e9b4c6bc31d7986ccf4e22d18af70e5508da/crates/rust-analyzer/src/handlers/request.rs)
在 IDE API 之外添加的 Cargo 根处理。核心通过 `crates_for` / `crate_root` 补齐身份；
不会把清单的导航位置转为 Rust 声明。旧 LSP 用 UTF-16 行列，原生 API 用字节范围，
后续图适配必须基于同一快照处理这个坐标差异。

这是同一固定 revision 的查询层对照，不是完整宿主 RustLanguageAnalyzer/CodeGraph 对照。
不能把这一小型 fixture 的一致性扩展为全部 Rust 语义等价。另运行系统 stable
rust-analyzer 1.97.1 作为辅助对照，同样得到这些查询结果；它不作为固定旧入口的主证据。
原始对照命令、JSON 和日志位置记录在 Task 1 report；临时 probe 没有进入正式交付。

### 后续证据缺口

完整 Rust 图、原文/规范化打印、跨提交身份、图外符号、测试过滤与全部旧回归仍需后续
适配和验收。当前核心只接收单个 Cargo workspace；rust-project.json、多独立 workspace
发现和公共 Host/ProcessTree/协议接入未在本任务实现。缺组件、依赖不完整或未知目标
会明确失败；没有长期缓存池、LSP 嵌套或成功空图回退。

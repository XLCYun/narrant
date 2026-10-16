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

### 公共查询事务异常隔离（第 3 轮修复）

固定 loader 可以将合法 `include_bytes!("bad.rs")` 的 `FF FE` 字节保留为 VFS 身份，
但它没有对应的 FileText。第 3 轮直接公共 API 回归复现 `load` 成功后 `source(id)`
向调用方泄漏 `Unable to fetch file text ... FileId(8)` panic：RED 为 exit 101，
1 失败、19 filtered，0.64 秒。首次测试先遇到 macOS `/var` 与 `/private/var` 路径
断言差异；改为对比 canonical 路径后才得到这份真实故障证据。

加载事务的边界现提取为一个私有 `native_transaction`。`source`、`parse`、
`definitions`、`outgoing_calls`、`parent_modules` 各包住整次实际原生查询；位置查询
所需的快照原文读取处于同一事务内，不再调用嵌套公共 `source` 边界。VFS 路径映射
仍允许资源身份，合法资源的加载能力没有改变。异常不变成空结果、不按诊断文本决定
语义，不增加文件分类层、磁盘重读、逐文件 panic 探测或上游补丁。

公共 API 回归实际请求二进制资源的五种语义查询，均取得带真实 FileText 诊断的 Err。
资源加载后在磁盘改成有效 Rust 文本仍返回 Err；正常 lib.rs 被改写后，其原文与
`target` 定义导航仍来自加载时快照。完整图调用方必须在任一查询 Err 后终止该次分析
并丢弃项目，不能继续查询、重试或拼接部分图；此接入义务留待 Task 5 验证。
取消测试保留 `Cancelled::{Local, PendingWrite, PropagatedPanic}` 的原始错误类型，
也覆盖 IDE 正常返回 Cancellable 错误的传播；这是边界分类证据，尚非真实协议取消验收。

第 3 轮命令前缀为 `PATH=/Users/lizhanfa/.nvm/versions/node/v24.18.0/bin:$PATH scripts/cargo-agent.sh`，
以下聚焦验证均 exit 0：

| 参数 | 实际结果 |
|---|---|
| `+1.98.0 test --manifest-path native-helpers/rust/Cargo.toml --lib --locked tests:: -- --skip loading::tests:: --nocapture` | 6 通过、15 filtered，0.79 秒；核心查询、快照和取消分类 |
| `+1.98.0 test --manifest-path native-helpers/rust/Cargo.toml --lib --locked invalid_utf8 -- --nocapture` | 2 通过、19 filtered，0.63 秒；根与块内源码失败 |
| `+1.98.0 test --manifest-path native-helpers/rust/Cargo.toml --lib --locked loading::tests::binary_resource_is_not_misclassified_as_module_source -- --exact --nocapture` | 1 通过、20 filtered，1.10 秒；md/rs/bin 合法资源 |
| `+1.98.0 test --manifest-path native-helpers/rust/Cargo.toml --lib --locked unreadable -- --nocapture` | 2 通过、19 filtered，2.77 秒；根模块与 11 种块内声明 |
| `+1.98.0 clippy --manifest-path native-helpers/rust/Cargo.toml --all-targets --locked -- -D warnings` | exit 0，2.89 秒 |
| `+1.98.0 fmt --manifest-path native-helpers/rust/Cargo.toml -- --check` | exit 0 |

合计 11 个受影响用例通过；实际故障输出在 `--nocapture` 日志中保留，未更改 panic hook。
首次核心筛选的 `tests::` 误包含 loading 测试，已用 SIGINT 中止并收窄为上表命令；
不把该中止运行计为通过，也未运行宿主全量测试或触发 quality-gate。原始命令、日志路径
与此轮文件清单追加在 ignored Task 1 report，完整图、双平台及性能验收范围不变。

### 后续证据缺口

完整 Rust 图、原文/规范化打印、跨提交身份、图外符号、测试过滤与全部旧回归仍需后续
适配和验收。当前核心只接收单个 Cargo workspace；rust-project.json、多独立 workspace
发现和公共 Host/ProcessTree/协议接入未在本任务实现。缺组件、依赖不完整或未知目标
会明确失败；没有长期缓存池、LSP 嵌套或成功空图回退。

## Task 2：直接内嵌 Python 核心（2026-10-11）

本轮建立 `native-helpers/python/` 独立 Rust 库、锁文件与核心测试，不切换宿主入口、
不生成完整 CodeGraph、不移植 ty LSP 生命周期，不删除旧实现或发布支持包。真实证据来自
macOS ARM64 / Rust 1.98.0；Windows、完整语言回归、公共 Host / ProcessTree / 协议、
支持包安装及 Review 接入仍未验收。

### 固定源码与版本对齐

生产核心的 `ty_project`、`ty_ide`、Ruff AST/codegen/db 等直接依赖全部固定到 ty `0.0.84`
对应 Ruff [`11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a`](https://github.com/astral-sh/ruff/tree/11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a)。
该 revision 的 Ruff crate 版本是 **0.0.14**，不是宿主正在使用的 registry **0.0.15**。
helper 从上游 Cargo.lock 建立传递依赖基线（包括 salsa 0.28.5），再解析自己的包与
测试依赖；没有修改上游源码或切换 revision。helper 的 rust-toolchain.toml 固定 1.98.0，
没有改全局工具链或被分析项目环境。

源码入口分别为原生
[`ProjectDatabase::fallible`](https://github.com/astral-sh/ruff/blob/11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a/crates/ty_project/src/db.rs)、
[`parsed_module`](https://github.com/astral-sh/ruff/blob/11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a/crates/ruff_db/src/parsed.rs) 和
[`ty_ide::goto_definition`](https://github.com/astral-sh/ruff/blob/11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a/crates/ty_ide/src/goto_definition.rs)。
AST 与导航使用同一个数据库、同一个显式项目 Program，未启动 ty server。
上游原始导航会映射 stub 到可找到的实现，本核心保留这个实际结果。

### 只读环境与来源

调用方显式传入单个已归一化项目根、有序源码根与 minor。所有根都是仓库相对目录。
加载先只读遍历项目根及明确源码根，校验物理位置留在仓库内，再把 `.py/.pyi` 和目录
以**词法相对路径**冻结到 MemoryFileSystem。`.git` / `.venv` 排除；逃逸或损坏链接、
可达 site-packages / dist-packages / pyvenv.cfg、I/O、非法 UTF-8 及非 UTF-8 编码声明
明确失败。内部链接不合并物理路径，也不重写模块身份；语义查询不再读取磁盘。
加载时要求调用方固定输入，复制过程不是应对并发改写的原子快照保证。

自定义 System 只暴露该内存快照：无用户环境变量、用户配置目录、缓存目录、可写接口、
命令执行器或可执行文件查找。ProjectMetadata 显式设 `UseUv::Off`、目标版本、
`python-platform=all`、源码根、空 extra-paths 与内存空 Python prefix，不进入解释器或
venv 发现分支。`ProjectMetadata::new` 不读取配置文件；本任务不重复实现宿主的配置、
PEP 723、跨项目冲突规则，后续适配器仍须按既有规则归一化后传入。

标准声明来自固定 revision 内嵌 typeshed；来源为 `stdlib/builtins.pyi` 等稳定路径。
明确的函数/类声明另由同源 AST 精确焦点匹配得到模块/词法限定名，例如
`builtins` / `len` 与 `builtins` / `str.upper`，不含 checkout、临时 URI 或行号。
其他声明保留原始来源和范围；完整外部符号身份仍属后续图投影验收。

### 真实查询通过

14 项必要全集通过，0 失败、0 ignored、0 filtered（1.64 秒）；其中一个隔离用例
另启动同一测试二进制的污染子进程，未启动 Python 或 ty server。

| 验证 | 实际结果 |
|---|---|
| 每个 3.7–3.14 minor | AST 的目标版本等于输入；本地 helper、len、str.upper 均实际导航 |
| 标准声明 | len 为 builtins/len；str.upper 两个原始 overload 候选均保留，映射到相同词法符号 |
| 别名与重绑定 | 无重绑定 alias 导航 other.py；有未知 RHS 的模块重绑定保留 main.py 与 other.py 两候选 |
| property / `.pyi` overload | property 导航 getter 原始声明；stub 两个 overload 范围保留，没有伪造实现 |
| `.py/.pyi` 并存 | 两个文件都可解析；实际导航 dual.py，与旧验收记录的行为一致 |
| 缺依赖 / 多平台 | absent.fetch 没有已知定义；Path.exists 保留多个原生标准声明候选 |
| Unicode / 换行 | 中文、emoji、BOM、LF/CRLF/单独 CR 保留原文；位置使用 UTF-8 字节边界 |
| 固定侧 | 加载后改写磁盘不改变原文或定义；未知文件、越界及字符内部位置返回错误 |
| 根与只读 | 明确根优先级决定 worker 模块；未列出的其他目录不可查询；文件路径和字节无变化 |
| 污染环境 | HOME/USERPROFILE/XDG/APPDATA、Python/Conda/TY_INSTALL_DIR/PATH 与仓库 ty.toml、.venv 未扩充上下文；len 正常 |
| 边界 | System 无读磁盘、写入或执行能力；逃逸链接与可达安装环境在原生加载前拒绝 |
| 内部链接 | alias/__init__.py、impl/__init__.py、api.pyi 各有词法路径；显式 import impl 仍导航 impl |
| 明确失败 | 不合法 UTF-8 / cookie、目标超范围、3.7 下 match 语法均失败，不变成空 AST 成功 |

`str.upper` 的两个焦点/完整字节范围为 `50727..50732` / `50709..50837` 和
`50860..50865` / `50842..50883`，各 minor 均观察到两者。stub overload 范围为
`42..47` / `28..67` 与 `82..87` / `68..107`。这些是固定 fixture 的导航证据，
不是持久身份字段，也未按同名删除候选。

重绑定 fixture 的赋值前后都返回 `main.py` 的 `114..119` / `114..129` 与
`other.py` 的 `4..10` / `0..18`。初始测试错误地假设 str.upper 单目标，以及重绑定后
不再包含原导入；真实结果暴露了测试假设问题。修正的是测试断言，生产核心始终保留
原生全集。核心没有“已确定运行时目标”字段；即使只返回一个候选，也不能据此抹去
缺依赖、装饰器或绑定反证。完整绑定/关系裁决必须由 Task 6 复用既有规则完成。

### 旧 Ruff 与新同源 API 的独立对照

测试依赖以独立别名引入 registry Ruff 0.0.15 的 parser/AST/codegen/source/text_size，
将相同原文分别交给**旧真实 parser/codegen**和**新数据库的 parsed_module/codegen**。
没有修改宿主实现，也没有以新引擎输出自证旧兼容。生产 normal dependency tree 中不含
registry Ruff 0.0.15，它只参与 dev 测试。

四项对照实际通过：类/字段/property/overload/显式 TypeAlias 的完整 AST 打印；
3.7–3.14 每个版本的代表语法（async、walrus、泛型、match、except*、类型参数、默认类型
参数、t-string）的声明头 clone/清空 body 投影及原文字节范围；字符串空白/docstring
换行差异；由 AtomicNodeIndex、ExprName、ExprContext 构成的合成有序声明锚点。
两版本在这些 fixture 的 `Mode::AstUnparse` 输出一致，字符串内双空格与文档字符串
换行变化仍改变比较内容。没有把这个接口/fixture 对照扩大为全部 Python 图语义等价。

最终命令（仓库根，前缀为
`PATH=/Users/lizhanfa/.nvm/versions/node/v24.18.0/bin:$PATH scripts/cargo-agent.sh`）：

| 参数 | 实际结果 |
|---|---|
| `+1.98.0 check --manifest-path native-helpers/python/Cargo.toml --lib --locked` | exit 0，1.55 秒；初步核心类型检查 |
| `+1.98.0 test --manifest-path native-helpers/python/Cargo.toml --lib --locked -- --nocapture` | exit 0；14 通过、0 失败/ignored，1.64 秒；本 helper 必要全集只运行一次 |
| `+1.98.0 clippy --manifest-path native-helpers/python/Cargo.toml --all-targets --locked -- -D warnings` | exit 0，27.45 秒 |
| `+1.98.0 fmt --manifest-path native-helpers/python/Cargo.toml -- --check` | exit 0 |
| `+1.98.0 tree --manifest-path native-helpers/python/Cargo.toml --locked --edges normal` | exit 0；生产依赖同源核对 |

原始命令、聚焦阶段、日志与自审记录保留在 ignored Task 2 report / scratch/python；
没有运行宿主全量、触发 quality-gate、改全局工具链或 push。Windows junction、取消与
协议错误分类、完整对象/内容/图/配置兼容性、双平台 helper/支持包/Review 仍须后续验收。

### Python 核心安装环境链接隔离修复（第 1 轮）

独立审查发现初版只对词法路径检查安装环境：普通仓库 `poison.py` 链接到
`.venv/lib/python3.14/site-packages/poison.py` 时，真实目标仍在仓库，能够绕过 `.venv`
遍历排除并进入语义上下文。审查经污染 fixture 的真实查询复现了 `poison.fetch` 被解析。
本轮新增直接加载回归，修复前同样返回了成功项目，测试为 exit 101、1 failed、14 filtered。

`check_path` 现在在 canonical 仓库边界通过后，继续对真实路径执行现有安装环境目录名
与祖先 `pyvenv.cfg` 检查。源码根和遍历项共用这个读取前检查；原有词法环境检查继续保留。
成功后仍按原词法相对路径冻结文件，没有修改合法链接的模块身份，没有增加回退或过滤结果。

新增回归覆盖指向 `.venv/site-packages` 的源码文件链接、目录名普通但祖先带
`pyvenv.cfg` 的文件链接，以及作为显式源码根传入的安装目录链接；三者均在数据库加载前
返回明确安装环境错误，磁盘 fixture 字节不变。合法 alias/impl 身份和逃逸链接、真实污染
子进程、直接可达安装环境拒绝、明确源码根顺序/只读共 5 个相关用例通过，0 failed/ignored。
只运行这些受影响用例，没有重跑 helper 或宿主全集。

命令前缀沿用上述 Cargo wrapper，各日志保留在 Task 2 report 的 scratch/python。
`tests::internal_links` 为 2 passed / 13 filtered（0.06 秒）；新增回归在补齐显式源码根
变体后精确复跑为 1 passed / 14 filtered（0.01 秒）。另外 3 个精确筛选用例分别为污染
环境 0.42 秒、直接安装环境 0.00 秒、源码根/只读 0.07 秒，均 exit 0。
Clippy `--all-targets --locked -- -D warnings` 为 exit 0（1.95 秒），fmt 检查为 exit 0。
审查的候选数量断言 Minor 按任务安排留待 Task 6/最终审查，不作为本轮修复范围。

## Rust Task 5 独立 helper 阶段（未生产切换）

基线 `332d7d3` 后完成 `narrant-rust` 单请求协议与完整图投影，继续使用固定
rust-analyzer revision `9074e9b4c6bc31d7986ccf4e22d18af70e5508da`。旧 syntax、test_code、
symbols、引用规则迁入 helper；原文、AST、位置和导航均来自同一数据库。公共 Host
完成最终图校验；helper 不实现第二图验证器或 LSP。生产工厂、迁移注册、支持包、
旧 Rust 链路和宿主 syntax 依赖未改动，本记录不代表完整 Task 5 或双平台验收。

多个独立项目使用固定上游公开 `to_crate_graph` / `CrateGraphBuilder::extend`、
`ProjectFolders` 与 VFS 原语合入一次数据库；关闭 watcher、build scripts 和宏服务，
沿用 Task 1 crate/block DefMap 完整性检查。显式 JSON 项目不生成 Cargo 容器，缺失
crate 根会终止整图。非空 standalone 保留旧实际入口的加载失败；空选集仍成功。
旧包实测独立 Cargo 与 JSON 的 health/parentModule 导航响应、standalone warning
保存在 ignored scratch；这些导航记录只证明装载边界，不能等同完整图 parity。
另外，旧实际分析器与新 helper 经公共 Host 对双别名 fixture 比较了完整图、diff、
根节点、外部符号和 source，确认 canonical 合并与各词法 origin/coverage。
共享输入层的修改仅增加三档 Rust profile 的物理别名准入，原四档语言策略保留。

真实 Cargo 验证使用 ignored scratch 中独立安装的 1.81.0、1.94.0、1.96.0 minimal
工具链；未改全局默认版本。每版覆盖有效/缺失/过期锁及 workspace 根/成员入口共
18 组，逐项比较分析前后目录字节；解析成功时还断言实际依赖边和 workspace 根。
1.81 有效锁成功，缺失或过期锁明确拒绝写回；1.94/1.96 全部成功，项目锁不变。
分析使用 offline；开发安装工具链与产品禁止自动下载分别处理。

1.94 实测暴露固定上游自动复制已有锁并追加 `--lockfile-path`，与显式临时锁冲突。
仅在 1.82..1.94 用公开 `MetadataCommand` → `CargoWorkspace::new` 补齐完整依赖，
保留原生发现的 manifest/workspace 根、Cargo executable/env、cfg/sysroot、features、
有效 target 和配置；原始 workspace 锁复制失败直接失败，metadata 失败不回退 no-deps。
其余版本分支不扩大改写。真实 1.94 的 optional feature、平台依赖过滤和自定义 JSON
目标来源测试通过。Cargo 仍自行合并配置；小型补偿只读取 `cargo config get --format
 toml --show-origin` 的 build.target。升级上游须复核这些公开组装 API 与 Cargo origin
格式；未复制 Cargo 解析器或完整 loader。

公共 Host 实测涵盖旧语言集成断言、三档依赖完整图、多项目工具链/sysroot 冲突、JSON、
standalone、别名、逃逸上下文链接，以及忙碌 helper 和实际活动 Cargo 探针子进程的取消。
运行时工具链测试使用会转发真实 Cargo/rustc 的日志代理，明确比较实际 stable sysroot
与版本，并验证用户覆盖优先于目录 1.98、无覆盖时目录 stable 生效；允许 Rustup 自身
将 stable 规范化成完整 triple。构建使用 1.98，未在 loader 中抹除运行时覆盖。

本机 macOS ARM64 的小型完整图 debug 单样本，standard-library 约 46.6 秒、
all-dependencies 约 45.1 秒。完整性检查会遍历标准库 crate/block DefMap 与 body store；
该成本继承 Task 1 的漏载故障边界，当前没有为耗时放松检查或增加缓存。
这些是 debug 单样本，不是优化支持包性能，也不可与旧 release 对比；后续 Rust 集成
必须在同样本记录真实优化支持包耗时与峰值内存，完成 Windows、安装、Review 图/diff、
生产取消和无下载/构建验收。详细聚焦测试命令、日志和自审见 ignored `task-5-report.md`。

### Rust helper 阶段审查修复第 1 轮

I1 修复终结前 stdin EOF 和运行中 cancel。helper 在 ready 前通过共享
`CurrentProcessDomain` 确认/建立专属域，独立控制线程处理 cancel/EOF。原生 loader
内部的阻塞命令无法协作取消时，直接终止自身域；不启动脱离外层 Host 的 worker。
Unix 复用自有 ProcessGroup，直接启动也不会终止 shell/测试进程组；Windows 复用
私有 Job 创建策略，将自身放入嵌套 Job，正常完成前解除 kill-on-close 以保留 exit 0。
控制线程对 completed 写入锁的等待也有界，stdout 背压不能阻止 EOF 清理。

公共 Host 对**已经记录取消**后的预期 EOF/退出映射为 cancelled；之前/随后已解码的
真实 completed(error)、无效协议和真正读写错误仍优先，未取消时缺终结或异常退出仍为
硬失败。未报告且恰与已发送 cancel 同时发生的原生崩溃无法仅从退出状态区分，此观察
边界不被写成额外故障识别能力；没有新增私有退出码或协议标记。

旧 executable 的真实运行中 EOF 测试先失败；修复后直接 executable 验证运行中
cancel、stdin EOF 时 helper 与实际工具链探针后代自行结束，测试的失败后兜底清理
不计为通过。正常 completed 不等 stdin EOF、stdout 背压 EOF 也有直接测试。
公共 Host 实测两项 Rust 取消及实际原生错误分类；共享 subprocess 10 项和 Unix
进程树三矩阵通过。Windows 当前域的真实嵌套/终止/正常退出仍属于后续平台 gate。

I2 将旧语法/测试过滤的七项缺失断言接到真实 helper + 公共 CodeGraph.diff：字符串
有意义空白、宏标点粘连、顶层宏实参、类型表达式中的独立嵌套声明、成员重排与闭包
使用顺序、通配导入下新增嵌套测试、嵌套测试增改后的父内容/包含关系。七项全部通过，
没有恢复旧 LSP 传输夹具。本轮未重跑无关的 18 个旧 Cargo 场景或 Host 全量。

## Python Task 6 独立 helper 阶段（未生产切换）

基线 `5cb3461` 后新增 `narrant-python` 单次原生 executable、严格 typed request 与完整
Python 图投影。Ruff/ty 仍固定 revision `11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a`；
旧 syntax/bindings/fields/known/project/reference 业务政策直接迁入 helper，Document 的 AST
与原文取自同一数据库，定义位置改为原生字节焦点。内嵌标准声明也由该数据库取得 AST，
不恢复 LSP、不加入第二个生产 parser，不读取磁盘解释器/stdlib、用户环境或执行项目。
选集与上下文分离；测试覆盖、配置来源和节点 origin 交公共 Host 做唯一最终图校验。

`python-static-v1` 只接受一个 `python-embedded-stdlib` 平台，version 为 `0.0.84`，
metadata 严格只含上述同源 `source_revision`，root 须包含当前 executable。
配置/PEP723/目标 3.7–3.14、有序源码根、多项目冲突政策沿用旧实现；加载前物理边界
和安装环境拒绝、加载后词法 alias 身份继续由 Task 2 核心保证。标准外部身份沿用
`python:stdlib:ty-0.0.84:`，导航的多个 overload 和重绑定反证仍须旧绑定/声明证明裁决。
Task 2 延后的断言现要求 `str.upper` 恰有两个不同焦点候选、`len` 一个，stub overload
恰有两个 `typed` 焦点；3.7–3.14 和绑定反例的聚焦测试通过，未删除原生候选。

经控制者确认，`native-helpers/common` 仅承载序列化 DTO、严格 cancel/PROTOCOL 与两门
helper 实际使用的单次控制循环。继续复用 `CurrentProcessDomain` 和现有 Unix/Windows
进程原语；没有分析引擎/Tauri 依赖、Host 图验证器、另一个进程管理器或通用插件框架。
Rust/Python 保留各自 typed request 与平台政策。抽取中修正旧 Rust 的初始解码失败会
伪造 `analysis-1` completed(error) 行为：非法 JSON/schema/envelope 现在不产生终结消息，
有效 analyze 的实际分析错误仍按原请求 ID 返回。两门真实入口均有此回归。

公共 Host 的 34 个 Python 聚焦测试全部通过（31.28s），迁移旧 graph/native 的业务
断言，覆盖自身内容/diff、执行顺序、接收者和表达式作用域、共享初始化、字段顺序、
版本/编码/Unicode、装饰器/property/overload、别名/内部链接、每个 minor 的标准关系、
多项目、污染环境、缺依赖与测试过滤。三个代表场景逐一比较真实旧 ty 完整图和新 Host
完整图：关系/装饰器/overload、绑定反证、独立项目搜索；比较 diff、根、External 描述及
每个对象的原文。旧 ty 为用户已有固定安装 `0.0.84`，仅只读调用，没有改动安装内容。
精确版本选择的旧初始化断言另改为实际数据库 AST target 与配置/PEP723 选择相等。
新增真实 Host 超时/捕获调用方 panic 也确认返回前 PID 已消失（1 test，两个场景，0.79s）。

Python 真实 executable 的活动分析 cancel、提前 stdin EOF、错误控制 ID、正常终结不等
EOF 和 stdout 背压 EOF 均通过；Rust 抽取后的严格 cancel、非法初始入口和四项实际生命周期
回归通过，后者含活动 Cargo 探针后代。新 Rust executable 经公共 Host 的代表性完整图
比较也通过（1 test，1.86s）。没有重跑无关的核心研究、完整 Host 或完整 Rust 语义套件。

本机 macOS ARM64 debug executable 的 3 项目、24 文件、600 方法单样本为 3.22s real，
最大 RSS 174800896 bytes（约 166.7 MiB），峰值 memory footprint 134316680 bytes；
图为 696 节点、24 根，exit 0。公共 Host 两次同规模 debug 样本约 3.219s / 3.209s。
这些不是优化支持包结果或旧 release 性能排名。详细命令、原始输出及中间失败保留在
ignored Task 6 report/scratch。

两门 helper 的 all-target Clippy `-D warnings` 通过。宿主 lib/tests 原始严格 Clippy
显示 Task 6 BASE 已有的重复 TS fixture 模块和 input.rs 两处 collapsible-if；本轮新增
fixture type-complexity 已用具名类型修正。仅在验证命令允许这两个基线 lint 类后通过；
未加入源码 suppress，也未修改无关宿主代码。基线告警仍是整分支清理与最终检查义务。

本记录仅代表独立阶段。生产 Python 工厂/注册、catalog、支持包发布、两平台实际安装与
Review、DTO/取消验收及旧 Python/ty/宿主 Ruff 退役仍按 TS/JS → Rust → Python 顺序完成；
本轮均未执行，完整 Task 6 与整项迁移尚未完成。

### Python 独立阶段审查修复

审查发现跨项目目标被调用方版本解析：Python 3.8 项目引用含现代语法的 3.14 项目时，
本应保留为未确定的关系会导致整图失败。直接跳过冲突目标又会漏检仅作上下文的非法源码。
实际旧、新入口对照确认，未选中的上下文也必须按目标自身版本检查普通及不支持的语法。

最终通过同一冻结数据库、同一 File 的原生 `PythonFile` / `parsed_module` 查询，显式使用
已经发现的目标项目版本。没有新增解析器、数据库或改变调用方默认版本；关系、装饰器和
继承仍使用原有跨项目冲突规则。选中文件与内嵌标准声明的既有检查保留。

`b63f055` 的四项真实 helper / 公共 Host 聚焦测试通过，包含有效上下文与选中目标的完整
旧、新图对照、装饰器/继承不确定关系，以及两种上下文错误和选中目标错误。另有一项原生
API 测试确认目标 AST 版本独立于调用方配置；Python 严格 Clippy 和两侧格式检查通过。
两轮修复均经独立复审，阻断项已关闭。预期 panic 测试在 `--nocapture` 日志中的输出噪声
留待最终审查；宿主严格 lint、生产切换及平台验收仍按上述阶段边界处理。

### 平台验收前的小范围清理

`aa5c09f` 将 configDir 测试夹具原样移到中立测试目录，三个旧、新消费者共用一次模块
声明；两处宿主验证条件做等价整理，并删除测试 helper 的四行不可达取消分支。
原有业务断言、短路求值顺序和错误信息保留，没有增加测试框架或运行时机制。

宿主 `clippy --lib --tests --locked -- -D warnings` 现已通过，未使用 allow 参数或源码
抑制；上述阶段遗留的重复模块与两处条件 lint 已关闭。七个实际聚焦测试覆盖输入与平台
校验、取消与错误优先级、后代清理，以及真实 TS/JS configDir 直接和继承场景，全部通过。
Rust 格式、指定 JS 文件 lint 与差异检查也通过；这批改动已经独立规格和质量审查。

这只是验收前整理。整项迁移仍需依次完成三门语言的生产接入、双平台安装与 Review、
旧链路删除、宿主全量测试及整分支审查。[草稿 PR #201](https://github.com/XLCYun/narrant/pull/201)
保留这些未完成项；GitHub Actions 的预算拒绝不算执行结果或平台验收证据。

## Rust Task 5 本地生产接入与退役（2026-10-11）

本节补充上面的独立 helper 阶段，按最新0025/0025a政策先完成 macOS ARM64 本地生产验收。
Rust 1.1.0 原生候选已经接入支持包安装与应用工厂；旧 Rust LSP 分析器、服务器分发及
Host `ra_ap_syntax` 依赖已删除。三档设置类型、共享图与进程消费者保留，Python 生产链未切换。
此结果不表示发布就绪：候选仍 `published=false`、`verifiedTargets=[]`，最终 Windows/macOS
CI 待全部开发、审查和本地验证完成后集中执行，未推送或触发 Actions/quality-gate。

- 退役前实际旧安装与新 helper 在多独立 Cargo、JSON、自定义根 × 三档范围 × 测试开关的
  18 组完整图、root、外部符号、原文与 diff 逐项一致；退役后保留同矩阵的业务断言。
  已验证的 Cargo 1.81/1.94/1.96 只读18组及共享生命周期实现未改动，沿用历史证据。
- 正常入口构建优化包、暂存与解压 smoke 后，通过实际归档隔离安装、生产工厂与 Review→DTO。
  Review 两侧各4节点、差异6项；公开 API 真实 graph/diff、三档只读、解析错误与活动探测取消通过。
  工厂冻结用户运行环境，按每侧真实项目目录检查工具链；不注入 helper 构建版本，不安装组件或运行构建脚本。
- 旧 syntax16项业务去向为15项实际 Host 图测试（UTF-16传输断言改为Unicode/CRLF原文及稳定身份）
  和1项 helper 同源 AST 字段类型测试。其他 test_code、引用、外部声明、crate根、选集范围断言保留于原生测试。
  新版 RA 文档注释不再属于 trivia，迁移断言发现了注释变化错误参与 diff；修复使用真实 Comment/DOC_COMMENT
  分类，保留原文，并确认字面量、宏标点和真正属性变化仍参与比较。保留了修复前失败日志。
- 最终归档7,280,548 bytes，SHA-256 `fadcbdd482607428718bb1088389eef1e2476feac2cc9d1c0fbf056956ee7522`；
  helper22,954,944 bytes，SHA-256 `cc42912f8e2bb11904841b35b41902ba25e8f6209f2494318063e16f07a51abb`。
  实际目标许可闭包206项（204外部crate与2本项目crate）；204外部项均附许可文本。
  缺原文件的14个精确锁定crate使用固定上游原许可，清单/出处/hash在 `native-helpers/rust/licenses/`
  及归档 `Rust-license-sources.json`，未猜测作者或版权。
- 代表性 release 单样本的20节点项目：仅仓库0.508s/峰值38,649,856 bytes，标准库5.032s/711,852,032 bytes，
  全部依赖5.275s/667,484,160 bytes。该计量来自文档注释修复前优化包；注释修复后、最终import格式化前的优化包同样20节点，额外复测遇到
  同时 Host 编译，时间/峰值分别2.829s/38,518,784、22.687s/530,808,832、11.734s/655,654,912 bytes。
  两次均以 `/usr/bin/time -l` 记录进程最大RSS，三档输入快照不变；无旧实现同口径对照，不宣称性能提升。
  标准库/全部依赖峰值仍是明确局限，本轮没有新增缓存或优化框架。

聚焦 helper/Host/工厂/安装/Review/脚本测试、check、严格 Clippy、fmt 与原始 ESLint 已通过；
Host 完整套件保留至 Task7。完整命令、数量、原始失败、产物身份与许可14项逐项出处保存在
`.superpowers/sdd/0025a-existing-native-helper-implementation-plan/task-5-local-retirement-report.md`。

## Python Task 6 本地生产接入与退役（2026-10-11）

Python `1.1.0` 原生候选已接入按需支持包与生产工厂，正常优化构建使用独立 Rust 1.98.0
工具链。运行仍为固定 ty `0.0.84` 的同源 Ruff revision `11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a`。
包内 `bin/narrant-python` 和控制身份复用公共 Host；内嵌标准声明 platform root 与工厂
唯一 trusted root 均为 `bin`。真实隔离安装首先发现两根不一致，随后修正工厂与打包配置，
没有放宽 Host 的单一安装根校验。

macOS ARM64 正常归档的原目录与提取目录 smoke 均通过；实际 HTTP 安装、重新打开安装、
使用中租约与生产工厂分析各通过。生产 Review 在两侧工作区中完成 modified / removed `.py/.pyi`、
标准库关系和 DTO 序列化（before 7、after 4、changes 5），语法失败与预先取消明确传播，
结束后工作区和租约均释放。公共真实图/diff 保留 Unicode 原文、上下文外部目标、内嵌 `len` 身份；
格式与注释变化无内容改动，字符串内部空白改变产生 Modified。

退役前 22 个真实 native 业务用例、14 个图业务用例以及实际进程超时/caught-panic 退出通过。
三组实际旧 ty 完整图逐一与原生图一致，现固定为 JSON 快照，继续比较 category/language/id/kind/name、
signature、规范化内容、原文、contains/references、extensions 和边界分类。旧 Ruff `0.0.15`
四个实测 AST 投影对照同样固定为快照，覆盖每个 minor 的头内容与原文范围，旧测试 parser 依赖删除。
旧 Python 分析子树、ty 服务下载/分发/验证链、宿主 Ruff/pep440 和无消费者 toml 直接依赖已移除。
共享 `source_text` 的零生产消费者 UTF-16 转换与 LSP cancellation 类型留给 Task 7 集中清理；
Host strict Clippy 在这项共享清理前尚不能作为通过门禁。

优化 helper 代表性 24 文件、3 项目、600 方法、696 节点分析：首轮 0.447 秒，第二轮 0.308 秒，
`/usr/bin/time -l` 整个 helper 的 peak RSS 分别 154189824 / 154140672 bytes。每次 ready/completed
身份一致、自然退出、源码 hash 未变；这不是旧 release 对照或跨平台性能改善结论。

正常打包复用 Rust 目标过滤的 Cargo 非 dev 许可闭包收集器：226 个 locked package，其中
224 个外部包；附带所有找到的原始通知与固定来源。collection_literals `1.0.2`、quote-use /
quote-use-macros `0.8.4` 仅按该确切版本发布源和原始 Cargo/VCS/SPDX=MIT 声明，附上明确标为
标准条款的官方 MIT permission/conditions/disclaimer；没有推断版权年份、权利人或把占位行作为通知。
seahash `4.1.0` 发布 crate 没有附通知，上游 GitLab 通知取回失败；它附带原始 Cargo authors/VCS
和同样标注的标准条款，不能据此断言上游仓库不存在通知或原始通知已完整。version-ranges `0.1.1`
附固定上游 commit 的原始 MPL 2.0 LICENSE。证据与 SHA-256 见 `native-helpers/python/licenses/sources.json`，
本记录不构成许可法律判断，publication 仍受最终 gate。

可复用本地入口为 `node scripts/build-language-support-packs.mjs --id python --native --output <artifacts>`
和 `node scripts/verify-native-analyzers.mjs <artifacts> --language python`；旧专用 verifier 委托该公共入口。
本轮只执行有意义的聚焦检查；未运行 Host 全量，未 push、触发 CI/quality-gate、上传或发布。
Windows x64 实际安装、Review 与最终集中 CI 尚待全部开发/审查/本地检查完成。

## 最终双平台验收与自托管切换（2026-10-15—16）

本节更新上文各开发阶段的待办状态，不把早期失败或未运行的用例改记为通过。
三门语言的生产切换及旧链路删除已经完成；所有七套分析器共用 Native Host，
不再保留宿主解析器、LSP 桥接或生产回退路径。开发阶段完整 Host 默认套件为
620 通过、0 失败，128 项明确要求实际工具或平台的用例保持忽略，并由专项验收补证。
严格 Clippy、类型检查、Lint、格式和生产构建通过；既有 Git 夹具警告及 Vite 大块提示保留。

### 语言验收

| 平台与范围 | 实际结果与证据 |
| --- | --- |
| Windows 独立打包 | [38621306352](https://github.com/XLCYun/narrant/actions/runs/38621306352/job/116049238636) 通过。 |
| Windows 七套 helper 完整验收 | [38640186807](https://github.com/XLCYun/narrant/actions/runs/38640186807) 为 24/25 组通过，Rust 路径断言失败。修复后的 [38656526779](https://github.com/XLCYun/narrant/actions/runs/38656526779) 全部 12 组 Rust/共享验收通过，覆盖 helper、Host、安装及 Review。 |
| macOS 七套 helper 完整验收 | `5e27cbf` 的 [38730611394](https://github.com/XLCYun/narrant/actions/runs/38730611394) 构建全部支持包，25 组中 23 组通过；C/C++ 测试缺少 CMake、Rust 测试依赖旧 HOME 路径，两组失败保留。 |
| macOS 修复后的补验收 | `2407a2e` 的 [Rust 38748302122](https://github.com/XLCYun/narrant/actions/runs/38748302122) 全部 12 组通过；[C/C++ 38748308424](https://github.com/XLCYun/narrant/actions/runs/38748308424) 全部 7 组通过，包含原失败用例及真实安装、生产工厂、公共图与差异检查。 |
| Windows C/C++ 构建工具补验收 | `2407a2e` 的 [38748498046](https://github.com/XLCYun/narrant/actions/runs/38748498046) 全部 7 组通过，helper 为 34 通过、1 项按平台跳过；共享进程、Host、安装工厂及公共图与差异检查通过。 |

补验收期间增加任务私有 CMake，Rust 测试改从实际 PATH 查找完整 rustup 代理目录；
语言分析核心未改动。其他语言的生产代码及验收命令未变，复用此前通过的证据。
平台跳过项、内部子进程夹具及未选择的语言用例均不算通过。
这里的真实平台是 Apple Silicon macOS 27 与 Windows 10 x64，不代表已实测其他系统版本。

### 当前桌面账户与空间策略

现有 Mac、Windows runner 已分别切换到原桌面账户，不增加系统、账户、登录会话或
runner 注册。工作目录、工具配置与缓存独立；Windows 真实 Listener 的 Git/npm 配置位于
CI HOME，个人 Git 文件元数据未变。Mac [38730333996](https://github.com/XLCYun/narrant/actions/runs/38730333996)
与 Windows [38748060555](https://github.com/XLCYun/narrant/actions/runs/38748060555) 轻量检查通过。
它们证明接入、环境及共享进程能力，不能代替语言或真实应用验收。

用户要求取消 Mac 固定 30 GiB 启动门槛，开始 hook 现只记录实际剩余空间。
经单独授权删除本任务已验收的 `src-tauri/target`，释放约 7 GiB；源码、个人应用与 CI 缓存保留。
此前两次补验收仅因空间门槛在 checkout 前停止，未执行测试；取消门槛后上述补验收通过。
完成 hook 仍只回收列明的 CI 生成目录并保留日志，不清理开发 worktree 或个人文件。

### 真实应用与发布边界

macOS 的独立身份与本地比较 E2E 实际运行 6 项、全部通过；测试前后核对的 7 个日常
应用文件没有变化。测试应用使用 `com.cyunlab.narrant.e2e`、无痕 WebView、临时数据库和仓库、
内存凭据，并禁用正式更新。

10 月 16 日恢复 Windows 会话后，已核对固定 `2407a2e` 的完整结果：独立临时 checkout
中的安装、Tauri 构建和测试退出码均为 0；`application-isolation.e2e.ts` 1 项及
`local-comparison.e2e.ts` 5 项全部通过。测试实际于 10 月 15 日 23:22（北京时间）开始，
两个 spec 共约 1 分钟完成。这是现有 Windows 10 桌面的手动真实应用验收，不是另一次 Actions 运行。
9 处已知个人配置及安装路径的元数据未变化，测试进程和 mock 服务均已退出，原 runner 在线空闲。
此后提交仅更新文档，不影响本次测试的生产代码或命令。

WDIO 日志保留 Windows 可执行位及未安装外部 `tauri-driver` 的诊断提示；配置使用的
embedded 驱动实际完成了测试，没有为消除提示安装另一套驱动。构建保留既有未使用导入警告。
原始日志和报告位于 Windows 的 `C:\Users\win10\ci-setup\wdio-2407a2e\logs\test.stdout.log`
及同目录上一级的 `verification.json`。后续清理已删除约 5.38 GiB 的编译中间文件；
剩余约 2.19 GiB 的 `target`/`dist` 最初被执行策略拒绝，随后已按用户授权完成清理并核对结果；
原始日志和验证报告保留。清理不改变已经完成的测试结果。

### 后续完整检查与升级隔离（2026-10-16）

用户另行授权本任务手动触发 `quality-gate`。
[`72bf467` 的完整运行](https://github.com/XLCYun/narrant/actions/runs/38868882310)
中 Rust、TypeScript jobs 通过：620 项 Rust、426 项前端和 241 项脚本测试通过，
128 项 Rust 显式忽略及 2 项脚本跳过不计为通过。Windows E2E 的 5 个 spec 通过，
宝宝模式用例因旧按钮选择器失败，macOS E2E 因依赖未运行；不能记为整套通过。

选择器改为真实可访问名称，并用本地挂起模型响应验证生成首帧与取消后的界面状态。
本地诊断确认取消信号已到达 Rust registry、OpenAI adapter 已退出，HTTP 连接仍可存活，
因此不以服务器连接数代替任务取消结果。语言切换不再无界等待两次动画帧，
改由调用方检查目标语言的实际控件，避免后台 WebView 暂停动画帧时卡住。
临时诊断已移除，未修改宝宝模式生产行为或增加测试窗口抢焦点配置。

[`e74a099` 的完整运行](https://github.com/XLCYun/narrant/actions/runs/38886815723)
中 Rust、TypeScript 及 Windows E2E 通过：622 项 Rust、426 项前端、242 项脚本测试和
全部 6 个 Windows spec 通过，128 项 Rust 忽略及 2 项脚本跳过不计为通过。
macOS 第一次在编译期间因合盖睡眠断联；仅重跑该 job 后完成构建，但 WDIO 在建立会话时
遇到 `UND_ERR_INVALID_ARG`，未执行应用断言。最小真实 WebDriver 会话复现确认 Node 24
全局代理 dispatcher 与 WDIO 自带 Undici 混用后拒绝 `Content-Length`，仅为 WDIO 设置
`NODE_USE_ENV_PROXY=0` 后通过；构建和下载的代理不变。

本机复测另发现分支 E2E 的全页面输入框定位会与前一个关闭中的弹层竞争。测试现通过
当前触发器的 `aria-controls` 限定搜索框与选项，选中后等待该弹层卸载；保留真实输入、
点击和选中结果断言。连续切换最小场景先失败，修复后 40 次切换通过。临时诊断已删除，
类型检查、Lint 及 Standards/Spec 复核通过；这些改动不改变产品行为。
修复后在继承 `NODE_USE_ENV_PROXY=1` 的本机环境中，仅由 WDIO 子命令覆盖为 0，完整
macOS 桌面 E2E 的 6 个 spec 全部通过（46 秒）。最终提交仍需由 Actions 独立检出验收。

独立的 `Narrant Upgrade Test` 已替代共享 hosts、系统证书信任与正式安装操作；
只给测试进程设置临时 TLS 和独立应用身份。macOS 的上一 Stable 升级实测发现共享
更新器的下载任务编号缺陷，修复后 92 项更新模块回归通过；旧版实际失败仍保留。
按用户随后决定，macOS 升级测试暂停，普通质量检查及 E2E 继续；Windows 仅验证
修复后同一源码的两个独立版本。执行范围与真实记录见
[Application Update 验收](../application-update-acceptance.md)。
这不豁免正式发布的双平台上一 Stable 门禁；未发布支持包、应用或修改 Stable，PR 保持草稿。

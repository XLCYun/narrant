# PR #31 Rust 结构分析耗时诊断

日期：2026-09-13

## 结论

主要耗时在 rust-analyzer 的项目准备和语义查询。按实际改动文件运行两侧分析和
graph diff，原始基线耗时 **124.6 秒**；添加日志后，同样的新 before 工作区耗时
**123.8 秒**。仅保留同一路径的 before 构建产物再次运行，耗时降到 **79.2 秒**。
后一次仍为每侧启动新的 rust-analyzer 进程，没有复用服务器内存。

本次交付是分阶段日志、可重复运行的分析入口和诊断结果，未改变分析范围、关系语义、
服务器配置或缓存生命周期。日志版本与基线的两侧图摘要完全一致。

## 输入与测量边界

- [PR #31](https://github.com/XLCYun/narrant/pull/31) 的 comparison-base：
  `a14f544a8f698903db911c2b10c8f46ccff631eb`。
- head：`6c7c55e8a2dd08eedf4eeff12f25a8b1578fdae4`。
- 共五个改动文件，其中三个 Rust 文件：`src-tauri/src/lib.rs`、
  `src-tauri/src/github_discovery.rs`、`src-tauri/tests/github_discovery.rs`。
- before 只有 `lib.rs` 实际存在；after 分析三个文件。两侧均使用默认
  `include_tests = false`。不按 Cargo 测试 target 整体排除文件。
- 复用应用已有、提交与工作目录状态已核对的 head；before 使用本次诊断专属的
  detached worktree，避免干扰应用工作区的租约与清理。
- Apple M2、8 个逻辑 CPU、8 GiB 内存、macOS 26.6.2；rust-analyzer 和 rustc
  均为 `1.97.1 (8bab26f4, 2026-07-14)`。
- 分析客户端使用 Debug example，调用与应用相同的公开 `RustLanguageAnalyzer`
  和 `CodeGraph::diff`；外部服务器使用本机安装包所用的 `.tools/rust-analyzer`
  启动器。该启动器的 Cargo 调用经过仓库全局锁与 sccache。
- 客户端编译在测量前完成，测量期间未同时启动 Agent Cargo 构建。新 before 的
  `target` 为空，但系统依赖下载缓存与 sccache 已存在；不称为全机器冷启动。
- 以下总计不含 UI、工作区租约等待、DTO 投影和 before 清理。新建 before 单独测得
  54.4 ms；重建操作（含删除旧工作区）测得 780.2 ms，最终单独清理为 734.0 ms。新增的应用层日志可继续测量
  这些入口阶段，不能将本次 example 的耗时视为安装包端到端延迟。

## 分阶段结果

下表是添加日志后的运行，单位为秒；就绪等待与构图是互斥的外层阶段。

| 阶段 | 新 before：before | 新 before：after | 保留构建产物：before | 保留构建产物：after |
| --- | ---: | ---: | ---: | ---: |
| 本地选中文件语法解析 | 0.049 | 0.048 | 0.044 | 0.047 |
| 启动服务器及 initialize | 0.193 | 0.168 | 0.182 | 0.157 |
| 等待项目就绪 | **50.873** | **10.071** | **9.125** | **9.065** |
| 语义查询与构图 | **30.195** | **31.993** | **29.209** | **31.229** |
| 关闭服务器 | 0.129 | 0.090 | 0.073 | 0.088 |
| 单侧总计 | **81.442** | **42.375** | **38.641** | **40.591** |

读取和末尾重新校验选中文件均为毫秒级。图 diff 在两次测量中均小于 1 ms。
before / after 分别产生 115 / 172 个节点、266 / 386 条引用；diff 有 59 个变化对象。
基线和两次日志运行的调试摘要分别为 `8e6523cf1e525cae`、`d3fb90a1e6fa6af6`。
摘要仅用于同一工具链下复跑核对，不是持久化标识。

### 项目准备：新增工作区会重复准备编译期依赖

rust-analyzer 的 work-done 进度显示，新 before 的
`Building compile-time-deps` 用了 **39.552 秒**；保留该工作区构建产物后降为
**1.802 秒**。after 已有构建产物，对应阶段分别为 2.550 / 1.856 秒。
日志还记录了 Fetching、CrateGraph、Roots Scanned 和 Loading proc-macros。
这些标题是服务器提供的阶段名；Fetching 不等同于全程在下载网络数据。

观测到服务器调用的是 workspace 范围的 Cargo compile-time-deps check，包含
`--workspace`、`--all-targets`、`--compile-time-deps`，并非只处理 diff 行。
现有 Rust Adapter 关闭了 checkOnSave、diagnostics 和 cachePriming；这些配置没有
阻止此次项目加载中的编译期依赖准备。

当前 [BeforeCheckout](../../src-tauri/src/structure_review/application/workspace.rs)
在分析后删除整个 before 工作区，其默认 target 也随之删除。因此，下一次实际
重新分析仍可能付出这笔代价。复跑证明保留构建产物有收益；该实验没有改动应用的
清理约定，也不代表缓存后所有分析都能省下固定的秒数。

### 语义查询：服务器就绪之后仍有大额请求等待

新 before 这一轮的 LSP 请求明细如下，时间包含客户端发送与等待响应，不是服务器
CPU 采样结果。它们已经包含在上面的外层阶段中，不能再与其相加。

| 请求 | before 次数 / 累计秒 | after 次数 / 累计秒 |
| --- | ---: | ---: |
| `experimental/parentModule` | 1 / **10.863** | 3 / **11.025** |
| `textDocument/definition` | 600 / **16.836** | 870 / **17.541** |
| `textDocument/prepareCallHierarchy` | 34 / 0.251 | 43 / 0.286 |
| `callHierarchy/outgoingCalls` | 34 / 0.317 | 43 / 0.435 |

每侧首个 parentModule 请求均接近 11 秒。definition 也各有一次约 10 秒的慢请求，
并非每一个查询都要几十毫秒。保留 target 后仍可复现这些慢请求：parentModule
约 10.75 / 10.98 秒，最慢 definition 约 10.03 / 9.68 秒。

这说明磁盘构建缓存与服务器的语义计算成本是两件事。当前每侧每次都新建服务器，
即使收到项目 quiescent 通知，随后请求也仍可能花费较长时间。将慢请求进一步归因到
rust-analyzer 内部的宏展开、名称解析或类型推导，需要服务器内部 profile；本次
请求日志不能证明其中某一项独占这些时间。

### 为什么 diff 小，仍会产生这么多查询

结构图按改动**文件**圈定，分析文件中的所有保留声明；它不只分析 diff 行。
PR31 的 `lib.rs` 仅增加 20 行，但 before 仍含 114 个保留声明，after 为 116 个。
这符合先生成完整对象图再做 graph diff 的现有接口约定。

[references.rs](../../src-tauri/src/structure_review/rust_analyzer/references.rs)
会按源码位置查询调用、写入、宏、实现和函数值候选引用。普通 `PATH_EXPR` 也先作为
`code.references` 候选查询定义，再根据目标类别排除 Type / Value。因此，虽然图里
没有普通 `code.reads` 或函数体 `type_use`，不意味着所有相关表达式都已避免查询。
本轮按位置的 definition 缓存命中为 0；各位置首次出现，最终引用去重不会减少这些
已经发生的 LSP 往返。

再增加用途统计、保留两侧构建产物复跑，耗时 **79.1 秒**，图摘要仍一致。该轮全部
definition 查询的两侧合计如下；耗时包含某些请求触发的首次语义计算，不只是通信。

| 定义查询用途 | 次数 | 累计秒 |
| --- | ---: | ---: |
| 调用 `code.calls` | 650 | **25.651** |
| 函数值等候选 `code.references` | 596 | **4.824** |
| 字段类型 `code.type_use` | 171 | 2.031 |
| 导入 `code.imports` | 28 | 0.104 |
| 实现、写入、宏使用 | 25 | 0.196 |

候选引用约占查询数量的 41%，但查询耗时主要仍在调用目标解析；不能认为删掉这些
候选就能省去大部分时间。该统计按查询用途计数，没有把候选进一步区分为最终保留
或丢弃，因此也不能将 596 次全部视为无用请求。

调用目标的图外描述还会读取未选文件：before / after 加载 32 / 52 个图外文件，
本地解析累计 **1.505 / 2.250 秒**。这不是主要瓶颈，但前端隐藏图外节点目前不会
消除该后端工作。新加入的字段 `type_use` 路径已在读取图外源码前截断，不应与此混淆。

## 后续优化的顺序

1. 为相同提交保留有界、可回收的 before 分析构建产物，或缓存已经生成的图。
   本次同路径复跑已经验证前者的收益。缓存需包含提交、工具链、分析器和选项等身份，
   不能让不同分支共用可变 target，也不能破坏现有工作区租约与回收保证。
2. 继续定位首次大额语义请求，再验证固定快照的服务器会话复用或其它语义准备方案，
   测量总耗时而不只看 wait_ready。会话复用的收益尚未实测；parentModule 同时支撑
   模块归属和跨文件测试排除，不能直接省掉。
3. 减少无用的 definition 候选查询，尤其是最终被过滤的数据值引用；必须保留函数值、
   别名和宏等现有语义，不能仅凭名称猜测或把 `code.references` 整体删除。
4. 最后再考虑图外描述的按需加载。本地 AST 解析与 graph diff 没有表现出优先优化价值。

这些是根据测量得到的下一步方向，本次没有修改这些行为。

## 日志与复跑方式

- `narrant::structure_review`：工作区等待、阻塞任务排队、before checkout、两侧读取与
  解析、服务器启动/就绪、构图、源码校验、关闭、graph diff、投影及清理；带 side、
  review/snapshot、文件数、节点/引用数等上下文。
- `narrant::lsp`：每种方法的次数、失败数、总耗时、平均值与最大值；超过 1 秒的请求
  单独输出方法和请求序号；标准 work-done 进度仅提取 title 和起止，忽略 message、
  token、请求参数及响应正文。服务器进度时间取自客户端收到通知的时刻。
- Rust 构图另输出每个文件的耗时、按关联用途统计的定义查询、图外文件数量及解析耗时。
- 沿用现有 Debug 终端 subscriber；Release 应用仍不自动安装 subscriber，也不新增
  日志文件。下面的 example 自带 subscriber，可独立捕获日志。

先构建 example，再直接运行生成的程序。不要用持有 Cargo 全局锁的 `cargo-agent.sh run`
运行分析，否则服务器内部的 Cargo 也需要同一把锁，会干扰测量。

```bash
scripts/cargo-agent.sh build --manifest-path src-tauri/Cargo.toml \
  --example profile_rust_analysis

# before_dir / head_dir 指向提交已核对的仓库；head 优先复用已有工作区。
RUST_LOG=narrant::structure_review=info,narrant::lsp=info \
NARRANT_RUST_ANALYZER="$PWD/.tools/rust-analyzer" \
  src-tauri/target/debug/examples/profile_rust_analysis \
  "$before_dir" "$head_dir" \
  src-tauri/src/lib.rs \
  src-tauri/src/github_discovery.rs \
  src-tauri/tests/github_discovery.rs \
  > .tools/pr31-analysis.log 2>&1
```

入口：[profile_rust_analysis.rs](../../src-tauri/examples/profile_rust_analysis.rs)。
每侧仅分析传入路径中实际存在的文件；重命名时调用方需同时提供前后路径。
缓存对照实验应复用同一 before 路径，仅改变是否保留构建产物；不要通过同时编译客户端
制造额外锁竞争。诊断完成后删除自己创建的 before，不清理应用所有的 head。

本机原始日志保存在被忽略的 `.tools/repros/pr31-performance/`：
`baseline.log`、`profiled-fresh.log`、`profiled-warm.log`、`profiled-purpose.log`
和对应耗时 JSON。
本报告保留核心数值，原始日志及缓存不进入提交。

## 验证

- LSP 聚焦测试 13 项通过，包含真实 stdio 交错通知、成功/失败请求汇总和参数/正文不输出。
  日志捕获测试在独立测试子进程中运行，避免其他并发测试的 tracing callsite 状态干扰。
- 结构分析聚焦测试 78 项通过、11 项显式忽略；真实 PR31 分析另由上面的入口执行。
- 所有目标 Clippy（`-D warnings`）和 rustfmt 检查通过。
- 原始基线、新 before、保留 before、按用途统计四次测量的两侧图摘要相同。

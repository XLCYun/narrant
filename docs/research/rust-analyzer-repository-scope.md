# rust-analyzer 限定仓库范围：PR31 实验

日期：2026-09-14（Asia/Shanghai）。分支：`codex/rust-analyzer-repository-scope`。

## 结果

在同一台机器上，PR31 两侧代码图及 graph diff 的分析从缓存后的 **73.36 秒**
降到 **4.25–4.97 秒**，三次样本的中位数为 **4.41 秒**，约减少 **94%**，
约为原来的 **1/16.7**。不包含 checkout 准备、客户端编译、UI 绘制或安装包启动。

显式节点、声明内容、包含关系和图内字段类型关系保留。但这不是语义等价的优化：
base 的图内关系由 46 条降为 32 条，head 由 67 条降为 51 条。
依赖外部类型信息的仓库内调用也会丢失。该分支用于验证这一取舍，不能据此宣称
只减少了图外目标的开销。

## 固定输入和对照方法

- 起点为 `5ab632d5f7d1790e8f6b33cac2fb4e7fb95add53`，它撤销了
  `e06891a` 的 perf 日志改动。与 perf 之前的 `b2d88aa` 比较，`src-tauri` 没有差异。
- PR31 base：`a14f544a8f698903db911c2b10c8f46ccff631eb`。
- PR31 head：`6c7c55e8a2dd08eedf4eeff12f25a8b1578fdae4`。
- 文件范围：`src-tauri/src/lib.rs`、`src-tauri/src/github_discovery.rs`、
  `src-tauri/tests/github_discovery.rs`。base 实际存在 1 个，head 存在 3 个。
- `include_tests=false`，两侧依次分析。复用应用已有 head 副本；只为本次实验创建
  detached base worktree，没有重复签出 head。
- rust-analyzer `1.97.1 (8bab26f4 2026-07-14)`；同一台 macOS arm64 机器、相同服务器启动器。
- 分阶段实验使用原 perf 提交留下的同一个 debug 客户端可执行文件；透明 stdio 代理
  只覆盖 initializationOptions。所有变体都经过该代理，且都设置 Cargo offline，
  因此不把网络状态差异计为优化收益。没有恢复生产代码中的 perf 日志。
- 单独通过公共 `LanguageAnalyzer` 导出完整图进行比较；该导出不进入性能样本。
  原完整配置与仅关闭 build scripts/过程宏的节点、引用和图摘要完全一致，
  因此图对照导出使用后者以避免再次生成编译产物。最终实现也重新导出两侧图，
  逐字段结果与 repository 变体相同，不仅检查节点总数。
- 测量期间没有并行启动多个分析器或本任务的 Cargo 编译。机器上的文件缓存、
  registry 和 sccache 不是全新状态。这是本机少量样本，不是延迟保证。

忽略目录 `.tools/repository-scope-perf/` 保存了输入、透明代理、运行脚本、阶段日志、
初始化选项、图导出和服务端审计。`run.py <variant> <label>` 重跑计时；变体为
`baseline`、`no-build`、`no-deps`、`repository`。它先校验两个工作副本的 HEAD，
直接运行已编译客户端；不要通过持有 Cargo 全局锁的 `cargo run` 启动它，
否则 rust-analyzer 的 Cargo 子进程会等待同一把锁。

本次创建的 base worktree 已在结束时回收；重跑前需在上述目录重新准备相同的 base：

```sh
git worktree add --detach .tools/repository-scope-perf/before a14f544a8f698903db911c2b10c8f46ccff631eb
```

应用的 head 工作副本始终保留，实验前后 HEAD 与 tracked worktree 状态均未改变。

## 逐步关闭加载内容

下表均为 before + after，总耗时包括图比较。表内服务端就绪和关系解析是顺序阶段，
与子级 progress 通知不同，不能把可能重叠的服务端 progress 时长再次累加。

| 配置 | 项目就绪 | 关系解析 | 总分析 |
| --- | ---: | ---: | ---: |
| 完整上下文，缓存后的基线 | 16.279 s | 56.733 s | 73.362 s |
| 再关闭 build scripts 和过程宏 | 15.591 s | 49.741 s | 65.559 s |
| 再设置 cargo.noDeps=true | 4.921 s | 5.892 s | 11.751 s |
| 再设置 cargo.sysroot=null，首次 | 3.322 s | 0.749 s | 4.251 s |
| 仓库范围，重复 1 | 3.648 s | 0.962 s | 4.971 s |
| 仓库范围，重复 2 | 3.473 s | 0.758 s | 4.405 s |

新 base 工作副本第一次跑完整模式用了 193.21 秒，head 47.52 秒，合计 240.74 秒。
其中 base 的 compile-time-deps progress 为 41.65 秒，还经历了多轮 Fetching。
不能用这次冷工作副本的数值直接计算配置提速；上表以同一副本缓存后的 73.36 秒为基线。
重复仓库范围实验前已删除本次 base 的 `target`，它仍无需重新编译依赖。

### 各阶段（中位数所在样本）

| 阶段 | 完整模式，两侧合计 | 仓库范围，两侧合计 |
| --- | ---: | ---: |
| 读取选中文件 | 0.003 s | <0.001 s |
| 本地语法提取 | 0.086 s | 0.076 s |
| 启动和初始化 LSP | 0.246 s | 0.089 s |
| 同步文档 | 0.002 s | 0.002 s |
| 等待项目就绪 | 16.279 s | 3.473 s |
| 查询定义、调用和父模块并组装图 | 56.733 s | 0.758 s |
| 校验源码和关闭会话 | 0.005 s | <0.01 s |
| graph diff | <0.001 s | <0.001 s |

两种配置都发出 1470 次定义查询，文件范围也相同。提速来自缩小服务端的分析上下文，
并伴随部分查询无法解析；不能归因为减少了 LSP 请求次数。
剩余开销主要仍在启动 Cargo 项目上下文，不在本地 AST 或 graph diff。

## 图结果与代价

| 项目 | base 完整 → 仓库范围 | head 完整 → 仓库范围 |
| --- | ---: | ---: |
| 显式节点 | 115 → 115 | 172 → 172 |
| 图内关系 | 46 → 32 | 67 → 51 |
| 已解析图外关系 | 183 → 11 | 271 → 14 |
| 未解析关系 | 37 → 205 | 48 → 326 |

节点 ID、名称、类别、签名、own code、extensions、contains 及虚拟根逐项相同。
前后图比较仍得到 59 个变更对象。三次仓库范围计时的图摘要完全相同。

所有丢失的图内关系都是调用或普通引用，没有丢失该 PR 的字段 `code.type_use`、
`code.applies_to`、`code.member_of` 等关系；也没有新增未经证实的图内关系。
例如 `Vec<DiscoveredPullRequest>` 中的图内类型仍可通过名字位置的定义查询定位。

具体的语义损失包括：

- 两侧各有 14 条相同的调用丢失，例如 `close_review → ReviewSessionStore::close`、
  `read_file_change → ReviewSessionStore::get`。接收者经过第三方 `tauri::State<T>`
  包装，服务器不再知道其解引用关系。
- head 另外丢失 `github_repositories → GitHubRepository::identity` 和
  `list_pull_requests → DiscoveredPullRequest::from`。前者依赖中间表达式的类型推断，
  后者需要标准库 `From` trait 语义。
- 未解析引用数量上升不代表多生成了代码对象；这些引用继续使用 Unresolved，
  当前前端按既有规则隐藏它们。
- 关闭 build scripts/过程宏还会失去生成代码、自定义 cfg 和过程宏展开的语义。
  `noDeps` 不提供 Cargo resolve/active-features 结果；多 package 仓库中，即使本地
  package 都已载入，跨 package 名称解析也可能缺失。不要把 noDeps 等同于
  “只去掉仓库外依赖、完整保留仓库内依赖关系”。

## 实现范围与验证

Rust 专用配置在 `rust_analyzer/lsp.rs`，不向通用 LSP 层或 Code Graph 接口加入 Cargo 选项。
同时设置 `CARGO_NET_OFFLINE=true`、`RUSTUP_AUTO_INSTALL=0`，作用域为分析进程。
适配器读取定义源码前检查规范化后的仓库边界，并移除标准库、registry、Git 依赖的
源码身份提取路径。仓库内未选中的上下文仍可解析为 External。

独立的 `analyzerStatus` / `viewCrateGraph(full=true)` 审计显示 PR31 的服务器状态为
`health=ok, quiescent=true`，载入 1 个 package、8 个本地 Cargo target，
crate 图没有 std/core/alloc 或第三方 crate。build script 作为一个本地 target 出现，
不表示执行了 build.rs。

这些配置控制正常 Cargo 项目的加载范围，不是文件系统沙箱；用户提供的
rust-project.json、外部 workspace 根和特殊链接配置需要额外核验，不能宣称已对
任意项目输入实现物理文件隔离。适配器自己的源码读取已覆盖仓库外文件和越界符号链接。

- 修改前模块测试：78 通过。
- 修改后模块测试：80 通过，12 项真实服务端/应用测试在普通测试中忽略。
- 单独串行运行真实 rust-analyzer 测试：11 通过，覆盖模块根、闭包、测试排除、
  图内字段类型、仓库内图外目标和仅修改函数体的比较。
- 新增真实测试使用无法获取的依赖版本和一旦执行就会写标记并 panic 的 build.rs：
  分析成功、未写标记、未生成 Cargo.lock，且直接图内调用和 `Vec<Item>` 字段关联成立。
- 新增 stdio 测试让定义指向非 UTF-8 仓库外文件及越界链接，验证它们不会被读取。
- 完整 `cargo test`：579 通过，12 忽略；rustfmt 检查和 all-target Clippy（warnings denied）通过。
  Cargo 验证通过仓库锁运行，真实服务端测试直接运行编译后的测试可执行文件，
  避免在外层持锁时启动同样需要 Cargo 的服务器。

## 官方依据

配置含义来自 [rust-analyzer 配置文档](https://rust-analyzer.github.io/book/configuration.html)：
`cargo.noDeps` 跳过依赖元数据；`cargo.sysroot=null` 关闭 sysroot；过程宏开关会影响
build scripts。健康状态不是每个符号均可解析的保证。
[服务端状态与调试请求](https://rust-analyzer.github.io/book/contributing/lsp-extensions.html)。

`noDeps` 下 Cargo resolve 为空时，依赖边和 active_features 不被填充，可查阅
[CargoWorkspace 源码](https://github.com/rust-lang/rust-analyzer/blob/master/crates/project-model/src/cargo_workspace.rs)。
该限制和本机实际丢失的关系均需在决定是否采用此模式时考虑。

# CodeGraph 独立项目迁移计划

2026-10-21：本计划的总 CLI 交付部分已由用户修订后的
[独立语言分析器计划](2026-10-21-independent-language-analyzers.md) 取代。
已完成提交与验证仍保留为历史基线，不重复实施；新验收不能沿用旧架构的 CI 结论。

Spec: [0027](../specs/0027-independent-codegraph-project.md)。关联 Narrant #205。

## Global Constraints

- CodeGraph 只提供图生成；图差异、Git、Review/Snapshot 与界面留在 Narrant。
- 所有消费者只走 CLI/stdio JSON，不公开 Rust 库或 HTTP 接口。
- 保留七套 helper（Rust、TS/JS、Python、Java、Go、C#、C/C++）、既有投影、来源、只读加载、测试开关、Rust 三档依赖政策和 macOS ARM64 / Windows x64。
- CodeGraph 自己持有构建所需源码和发布产物；不得通过路径依赖 Narrant。Narrant 使用经过验证的固定发布版本。
- 不删除或弱化既有语义和生命周期验收。方法、结构体、类添加易读中文注释；Rust 测试位于独立 tests/ 文件。
- Cargo 始终通过所属仓库的 scripts/cargo-agent.sh，保留增量与独立 target；实现任务串行，禁止并发冷编译。
- 实现者只在指定分支提交代码，不操作 GitHub、不发布、不合并、不再委派 subagent。控制端安排审查与发布；用户人工合并两个仓库的 PR。
- Narrant 不改应用身份，不手动触发 quality-gate，不使用 skip-ci。最终以最新 head Complete CI 和 CodeGraph 双平台验收为准。

## 交付顺序

先建立 CodeGraph 源码与公共契约，再建立独立包和 CI，最后让 Narrant 只消费 CLI。
可以在发布前用受控候选包进行本地集成开发；生产版本标记只有在真实发布验证后才更新。
CodeGraph PR 审核合并是首次 Release 的前置条件，Narrant 最终 CI 必须使用精确版本产物。
所有源码先做可审查实现，遇到该发布前置条件时保留实现与证据，等待人工合并。

## Task 1 — 独立引擎、统一 CLI 与协议

工作目录 `/Users/lizhanfa/projects/CodeGraph`，分支 `codex/issue-205-standalone-engine`。
来源 `/Users/lizhanfa/.codex/worktrees/688d/narrant`，基线 `890ae4960aba2fc64946423093ff064c41d6ebf7`。
只修改 CodeGraph；Narrant 在此步骤作为只读来源。遵守 Global Constraints（本任务摘要：
不迁 diff、不公开 Rust lib/HTTP、保留七套语义、中文注释及独立测试、Cargo wrapper、不委派、只本地提交）。

1. 迁入 tracked `native-helpers/` 七套实现、common、Windows probe 和对应测试；保留上游固定版本及第三方许可证。
   把所需 `child_process` 平台实现和测试归入 CodeGraph 自有源码，修复 common/probe 对 `src-tauri` 的跨仓引用。
   工具目录和编译产物不复制。保留已有私有 helper 名称和 `narrant.native-analyzer/1` 以减少纯改名风险，公共入口必须叫 CodeGraph。
2. 建立内部模块 `code_graph`（生成/校验部分，排除 diff、CompareError 与差异测试）、`native_analyzer`、
   `native_languages`、`child_process`，从 Narrant 提取 Host、来源校验、scratch、限制、目标 SDK 和 Rust 策略。
   去除 Tauri、Review、language_support 安装服务依赖；支持包 ID 成为 CodeGraph 内部小枚举。
   根 Rust package 为 binary-only `codegraph`，初始引擎版本 `0.1.0`，内部模块不发布 crates。
3. 增加 `codegraph analyze --pack <id> --pack-root <absolute-path>`，以及 `--help` / `--version`。
   ID 沿用 `rust`、`typescript-javascript`、`python`、`java`、`go`、`csharp`、`c-cpp`。
   受信支持包由调用方选定；请求不得携带任意 helper 命令、执行环境或未校验 SDK 根。
   CLI 负责完整 factory 与分析调用，Narrant 不再需要知道语言运行时布局。
   `control/codegraph.json` 独立声明 `schema_version: 1`、`pack_id`、`pack_version`、`engine_version`、`protocol`；
   CLI 核对包 ID、引擎版本和协议，首版两个版本均 `0.1.0`。原 `control/native.json` 保持私有且不混入公共字段。
4. 公共协议 `codegraph.analyzer/1`，UTF-8 NDJSON。启动输出 ready（协议、引擎版本、包 ID）；
   消费者发送一次 analyze（请求 ID、绝对 repository_root、相对 files、options：include_tests 与 rust_dependency_scope）。
   按请求 ID 接受 cancel；完成一次后退出。允许请求后的 stdin EOF，便于 `< request.json` 与管道使用；
   截断 JSON、重复请求、未知/重复字段、错误 ID 与协议不匹配均明确失败。
   completed 的 outcome 为 ok/error/cancelled；成功包含已验证图，失败有稳定错误类别和说明。
   stdout 不混入日志；普通消费者用显式 cancel，持有子进程所有权的消费者还必须能强制回收进程树。
5. 图 JSON 沿用 helper 图数据字段及六类语义（root_contains、nodes、external_symbols），
   返回前执行来源和结构验证。公开结果不泄露内部 scratch、SDK 绝对路径或私有 evidence；
   source 与语义 extensions 按既有契约保留。消费者仍对边界输入反序列化及构图验证。
6. 最大请求与结果各 128 MiB，stderr 256 KiB；遵循既有 60 秒启动、30 分钟分析、
   2 秒取消宽限、5 秒退出界限。重复/未知字段严格校验要递归生效，未知图 extensions 值仍按既有规范处理。
   CLI 阻塞输入/输出、嵌套 helper 进程、取消/异常退出和 parent kill 路径均需有界，不遗留工作线程或后代。
7. 迁入并适配既有图生成、Host、factory 和进程测试；新建 CLI 协议与真实子进程生命周期测试。
   语义集成 fixtures 可在任务 2 配齐真实包后运行；本任务必须执行 graph/Host/CLI 聚焦测试和 common 检查，
   覆盖合法空选集、错误与截断、版本不匹配、取消和清理，并区分已运行与待真实包验证。
8. 带入最小独立 `scripts/cargo-agent.sh` 与 cache 依赖（不能指向 Narrant），以及必要测试依赖/锁文件。
   根 target 独立；不要直接运行 cargo，不得通过完整 Tauri 依赖实现独立 Host。
9. 写 `docs/protocol.md`、独立构建/CLI 使用 README 和迁移出处；记录复制文件与必要变更的可比对清单，
   让审查区分完整迁移和新增行为。命令及 JSON 示例需实际运行；文档明确本阶段尚待发布。

提交并自审；报告包括 commit、命令/结果、未运行项及原因、供下游使用的准确协议与包布局文件链接。

## Task 2 — 独立构建、语言包、双平台 CI 与 Release

工作目录 `/Users/lizhanfa/projects/CodeGraph`，沿用任务 1 分支。任务 1 的 `docs/protocol.md` 是接口依据。
来源 Narrant checkout 只读。遵守 Global Constraints：七套能力与双平台完整保留、不迁 diff、中文注释、
独立测试、Cargo wrapper、不委派、不自行 GitHub 写入/发布/合并。

1. 迁入并适配全部语言构建、固定工具链配置、许可收集、可移植归档和解包验收脚本；
   独立 Node 工程只保留实际依赖。清除 src-tauri、Narrant app 或来源 checkout 的运行依赖。
2. 每个可选语言包含同一版本的 `bin/codegraph`（Windows `.exe`）与其语言私有 helper/runtime。
   保留 manifest 的身份、平台、每文件 hash/权限和归档 hash/size 校验；增加明确 CLI 版本/协议控制信息。
   另外发布相同 binary 的独立 CLI 归档供手工消费者配合 pack-root 使用。
   包版本与 CLI 版本明确绑定，首个 CodeGraph 系列统一 `0.1.0`，不能覆盖原渠道包。
   实际归档需满足消费者的 256 MiB 归档、1 GiB 解包及 2 MiB manifest 上限，超限应说明并有依据地调整契约。
3. 创建独立双平台 PR CI：macOS ARM64 与 Windows x64，构建、解包和真实公共 CLI 验收覆盖全部七包；
   保留既有语义、配置、来源、只读、生命周期、Rust 依赖档与 Windows process probe 测试。
   原 Narrant Review/Diff 专属测试留待任务 3，在 CodeGraph 以公共 CLI 图验收替代应用依赖。
   不能因为旧 workflow 只收集三包，就漏发另外四包。
4. Release 工作流只发布已验证精确提交，不覆盖不同内容的已有版本。
   一个 `v<engine-version>` Release 同时容纳双平台 CLI 和七套包；首版 tag 为 `v0.1.0`。
   语言资产为 `<id>-<target>-pack.tar.gz` 与 `<id>-<target>-manifest.json`；
   CLI 资产为 `codegraph-<target>.tar.gz`，发布 `SHA256SUMS`。
   准备全部双平台 CLI/包/manifest/校验信息，远端发布由控制端在 PR 合并及 CI 满足后执行。
   首版可为不可变 prerelease，不能冒充不存在的可下载版本；文档准确说明工具链前置与安装方式。
5. 运行脚本测试、格式/依赖验证和本机可覆盖的真实包构建/CLI 验收，记录 Windows CI 与本机缺项。
   更新迁移清单，列出每个原验收的新归属；交付发布与消费说明，包括 Narrant 所需精确 URL 规则和版本字段。

提交并自审；报告明确未发布状态、待 CI 项及产物契约。不关闭或创建 Issue。

## Task 3 — Narrant 统一 CLI 消费与迁移收尾

工作目录 `/Users/lizhanfa/.codex/worktrees/688d/narrant`，分支 `codex/issue-205-codegraph-extraction`。
CodeGraph checkout 作为只读协议及候选来源。遵守 Global Constraints；尤其保留应用身份、diff、
工作区保护、leases、七种语言与双平台测试；Cargo wrapper；不手动 quality-gate；不委派、不发布或合并。

1. 新建 `structure_review/codegraph_client`：单一 CLI 适配负责 exact 引擎/协议握手、请求、取消、
   有界 NDJSON、stderr 与进程树。返回图进入现有 code_graph 构造器，失败不能返回空图。
   测试覆盖伪 CLI 的错误/截断/身份不符/取消/后代清理及真实公共 CLI。
   现有共享 `child_process` 需带回 CodeGraph 已验证的 Unix 后代快照与出生时间识别，
   在外层 CLI 异常退出或强制终止时等待嵌套 worker/helper 回收，再释放工作区和语言包租约。
   保留 Git 等原有调用者，并运行共享进程与工作区生命周期的聚焦回归。
2. application 从已持有 lease 的语言包根启动 bin/codegraph，只传包 ID、根、源码选集和分析选项；
   不再装配语言 helper、平台目录或 SDK。保留两侧取消、Review Workspace 保护、图合并与 diff。
3. 更新语言支持目录/下载源/必需文件为固定 CodeGraph 产物，保留完整性、版本隔离与租约。
   生产不接受浮动 tag/latest。未发布前保持 unpublished，支持受控本机已验证候选集成，
   最终发布后将精确版本设为 published；不以 skip 或假请求替代真实下载。
   测试同 ID/版本的旧 Narrant 安装不能冒充 CodeGraph；保持旧安装使用锁，不能覆盖在用目录。
4. 移除迁出的 native-helpers、native Host/factory 和 CodeGraph 拥有的构建发布脚本，
   不删除 Narrant 共享 child_process 或图/diff；测试和 fixtures 按所有权迁移，保留消费验收。
5. CI 取得精确 CodeGraph 七包，在双平台完成安装、真实 CLI、图差异和 Review DTO 链路。
   Complete CI 继续汇总必要检查；CodeGraph 源码测试由其 CI 承担，记录验收映射，避免重复构建 helper。
   更新旧规范的实现归属、开发/发布文档、目录与命令引用。
6. 执行本地格式、类型、Lint、脚本及受影响 Rust 检查和可用的真实 fixtures；无 UI 改动则不增加浏览器验收。
   若有布局/交互改动按 AGENTS 另做真实浏览器检查。区分待发布/CI 事项，不自行声称 #205 完成。

提交并自审；报告标明任务 1/2 契约冲突，由控制端裁定并回到对应实现者修复。

## Task 4 — 发布后的固定版本与跨仓验收证据

前置：CodeGraph PR 经用户合并，精确提交双平台通过，控制端已执行授权首版发布并验证下载。
只在该条件满足后执行，不能把时间流逝当作合并或发布授权。

在 Narrant 分支完成最终精确版本/哈希/发布状态绑定，真实下载并执行生产安装与 CLI 图/Review 流程；
修复必要兼容问题并跑受影响验证。更新迁移验收记录，列明两个仓库 commit、release、产物摘要及结果。
遵守 Global Constraints，不重新引入 helper 源码、不跳过双平台 CI。Narrant PR 最新 Complete CI
通过后等待用户审核合并；全部验收与收尾满足后才由控制端完成 #205。

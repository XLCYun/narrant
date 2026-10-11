# Language Support Packs（语言支持包）

Status: Accepted — the user confirmed the consolidated design and requested
implementation on the isolated worktree, followed by a Pull Request targeting main.

## 现行打包实现（2026-10-11）

按 [ADR-0035](../adr/0035-migrate-existing-analyzers-to-native-apis.md) 与
[0025](0025-migrate-existing-analyzers-to-native-helpers.md)，七套支持包均经公共 Native Host。
Rust/Python/TS-JS 的 1.1.0 候选分别包含 `narrant-rust`、内嵌 typeshed 的 `narrant-python`、
私有 Node 24.18.0 和直接 TypeScript 6.0.3 API helper；不包含旧 rust-analyzer/ty/TLS 服务入口。
TS 分发删除 tsserver/typingsInstaller/watchGuard，保留固定标准声明闭包及 Node/依赖许可。
Rust/Python 的独立锁文件固定同批原生库，正常构建收集实际目标 Cargo 许可闭包。
Python 少数确切版本的 MIT 声明附上标明来源的标准条款；seahash 上游通知未取回的
边界见 [来源记录](../../native-helpers/python/licenses/sources.json)，不能声称通知完整。
Host Cargo 与应用 pnpm 锁不再含旧直接 parser/server 依赖；其他实际消费者的共享依赖保留。

三种迁移语言本地 macOS 验收通过，最终 Windows x64 和七套双平台 CI 尚待完成；catalog
保持 `published:false` / `verifiedTargets:[]`。以下首版决策与依赖调查保留时间背景，
服务器/内嵌 bridge 字样不代表现行生产 fallback。包租约、下载和项目工具链边界保持不变。

## 已明确的需求

- 在新的 worktree 分支设计和实现，基于上一轮已经完成的 Review 修复与结构图改动。
- 为 Rust、TypeScript 和 JavaScript 的结构分析提供可下载、解压并安装到指定位置的支持包。
- 需要分析某种语言时检查所需支持包；缺少时自动获取，安装完成后用于分析。
- 界面说明缺少哪些支持包、正在下载哪些支持包，并让用户理解当前准备进度。

## 已确认的决策

- Q1：正式名称为“语言支持包”（Language Support Pack）。结构分析是首个使用方；
  后续 AI 分析等需要语言能力的功能也可以复用，因此产品概念不限定为结构分析专用包。
  术语定义已写入 [`GLOSSARY.md`](../../GLOSSARY.md)。
- Q2：首版在用户进入“结构改动”、确实需要分析时检查并自动获取缺少的包，
  不在打开 Review 时提前下载。
- Q3：首版提供 Rust 包和 TypeScript / JavaScript 共用包。TS/JS 包包含独立 Node、
  TypeScript（含 tsserver）和所需语言服务依赖，用户不需要另装 Node。
- Q4：Rust 包提供 rust-analyzer，复用用户已有且与项目匹配的 Cargo/rustc 等工具链；
  当前分析范围需要 rust-src 时也使用已有组件。工具链或组件缺少时明确提示，
  Narrant 不替用户自动安装或修改 Rust 项目工具链。
- Q5：支持包独立编号，由应用声明需要的已验证版本。相同版本可跨 Review 和兼容的
  应用版本重复使用，缺少所需版本时才下载；支持包不自行追踪或升级到最新版。
- Q6：复用现有 `updates.cyunlab.com` OSS 分发渠道，为语言支持包使用独立路径。
  首批覆盖现有安装包平台：macOS Apple Silicon 和 Windows x64。
- Q7：离开结构视图或切换 Review 后，已经开始的下载继续在后台运行；多个 Review
  请求同一个包的相同版本时共用一次下载。退出应用时停止未完成的下载，已经安装
  完成的包保留。用户提出用临时目录承载下载和解压；临时目录的价值与异常退出
  清理的工程方案见下文。
- Q8：任一所需支持包下载失败，或缺少必要的 Rust 项目工具链时，停止此次完整结构
  比较并明确说明原因、提供重试；保留已经安装成功的包，File Changes 继续可用。
  不将部分语言的分析结果伪装为完整成功结果。
- Q9：首版清理未完成下载，重试时重新下载，不做断点续传；已安装成功的包复用。
- Q10：在结构改动页逐包显示缺少、下载进度、校验、安装和失败重试状态，全部准备
  完成后自动继续分析；离开后用轻量全局状态提示后台下载。用户补充：进度实现
  尽量简单简洁，优先考虑前端轮询，避免增加复杂度。
- Q11：支持包保存在当前用户的应用数据目录，跨仓库和 Review 共用。设置中增加
  “语言支持包”，显示状态、版本和磁盘占用，允许删除当前未被使用的包。首版自动
  清理临时残留，已安装的历史版本由用户手动清理，保留应用回退时的复用能力。

包与项目工具链的边界和版本策略记录于
[ADR-0032](../adr/0032-separate-language-support-packs-from-project-toolchains.md)。

## 设计时的实现背景

- 结构分析已经通过 `useStructureReview` 的 `enabled` 参数按需启动，结果按 Review
  Snapshot 缓存。当前状态只有 idle、loading、cancelled、failed 和 ready，没有支持包
  准备阶段。见 [`use-structure-review.ts`](../../src/structure-review/use-structure-review.ts)
  和 [`types.ts`](../../src/structure-review/types.ts)。
- 后端按比较两侧的文件分别识别 Rust 与共享 TS/JS 语言组；无适用文件时跳过语言工具。
  当前任一语言分析失败都会使整个结构比较失败，不能静默输出不完整的成功结果。见
  [`application/mod.rs`](../../src-tauri/src/structure_review/application/mod.rs)。
- 当前安装包目标为 macOS Apple Silicon 和 Windows x64。应用更新使用
  `updates.cyunlab.com` 的 OSS 通道；Q6 已确定支持包也使用该渠道的独立路径。见
  [ADR-0026](../adr/0026-publish-binary-releases-from-source-tags.md) 和
  [ADR-0029](../adr/0029-use-oss-as-the-stable-application-update-authority.md)。
- 当前 Tauri bundle 没有携带 Rust 或 TS/JS 的结构分析工具；默认路径来自编译时开发
  checkout 的 `.tools`，另有环境变量覆盖。按需安装需要增加用户机器上的持久化工具
  发现入口。Monaco 自带的 TS/JS 编辑器 worker 与结构分析工具链是不同用途。

### TypeScript / JavaScript 依赖调查

- 两种语言共用语义项目，适合共同提供一个包。现有分析器要求独立 Node、TypeScript
  `6.0.3`、typescript-language-server `6.0.0`、`@types/node 22.20.1` 和
  `undici-types 6.21.0`。
- Node 当前只校验 `>=22.22.2`；开发 setup 复制执行脚本的 Node，尚未固定其精确版本。
  TypeScript 包本身已经包含 tsserver 和标准声明库。TLS 提供现有传输使用的 LSP 接口，
  两个声明包用于语义上下文，并不是 Node 可执行文件自身的运行依赖。
- 运行分析不需要用户先安装 npm/pnpm，也不安装被审查项目的依赖。项目自身的库缺失
  与分析工具缺失是不同情况：前者可能产生未解析关系，后者当前导致分析失败。
- 桥接脚本内嵌在应用中，并与已验证的 TypeScript 引擎协作；支持包独立追踪最新版
  需要额外解决兼容性，不能只替换压缩包。
- 代码依据：[工具校验](../../src-tauri/src/structure_review/typescript_analyzer/tools.rs)、
  [开发 setup](../../scripts/setup-typescript-analyzer.mjs)、
  [内嵌运行时](../../src-tauri/src/structure_review/typescript_analyzer/runtime.rs)。
- 上游依据：[微软 tsserver 说明](https://github.com/microsoft/TypeScript/wiki/Standalone-Server-(tsserver))、
  [TLS 6.0.0 元数据](https://registry.npmjs.org/typescript-language-server/6.0.0)、
  [Node 声明元数据](https://registry.npmjs.org/@types%2fnode/22.20.1)。

### Rust 依赖调查

- 现有 Rust 分析器使用 rust-analyzer；Cargo 项目分析还依赖项目匹配的 Cargo/rustc，
  标准库分析涉及 rust-src。依据 Q4，首版复用已有工具链，不将这部分纳入支持包。
- 现行规格声明离线分析，并在进程环境中设置 `CARGO_NET_OFFLINE=true` 和
  `RUSTUP_AUTO_INSTALL=0`。但 rustup 对后者的定义只覆盖缺失 active toolchain 的
  自动安装；当前所验证 rust-analyzer 的源码仍存在显式执行
  `rustup component add rust-src` 的分支。仅凭这两个变量不能证明不会安装组件。
  实现通过项目目录内的只读探测检查已有工具链；需要标准库时，向服务器显式传入
  已存在的 sysroot 与 rust-src 路径，缺少时在启动服务器前失败。仅仓库范围传入
  `cargo.sysroot=null`。隔离工具链测试覆盖了缺少组件时不执行安装的行为。
- 首版在一个服务器会话中使用一套标准库。发现多个独立 Rust 项目且 sysroot 不同
  时，标准库/所有依赖范围会明确报错并提示切换为仅仓库；不会静默使用错误工具链。
- 依据：[现行 Rust 规格](0019-rust-language-analyzer.md)、
  [rustup 环境变量说明](https://rust-lang.github.io/rustup/environment-variables.html)、
  [已验证 rust-analyzer 版本的 sysroot 处理](https://github.com/rust-lang/rust-analyzer/blob/9074e9b4c6bc31d7986ccf4e22d18af70e5508da/crates/project-model/src/sysroot.rs#L429-L440)。

### 临时下载与完整安装

用户建议先下载到临时目录，完成后解压并放入正式位置。经核实，`tempfile::TempDir`
适合管理该临时目录，但自动清理有明确边界：

- 对象正常析构时递归清理临时目录；`Drop` 忽略删除错误，显式调用 `close()` 才能
  检查清理是否成功。清理前必须结束相关工作并释放文件句柄。
- 强制终止、崩溃和 `process::exit()` 等跳过析构的退出路径，不能依赖 `Drop` 清理。
  应用正常退出也应显式停止并收尾下载，而不是假定退出事件必然触发所有析构。
- 默认 `tempdir()` 使用系统临时位置，不能保证与安装目录属于同一文件系统；
  `tempdir_in()` 可以在支持包根目录内创建暂存目录，便于最后整体移动。
- 依据：[TempDir 清理语义](https://docs.rs/tempfile/latest/tempfile/struct.TempDir.html#method.close)、
  [Rust 退出与析构](https://doc.rust-lang.org/reference/destructors.html#process-termination-without-unwinding)、
  [tempdir_in](https://docs.rs/tempfile/latest/tempfile/fn.tempdir_in.html)、
  [rename 的文件系统限制](https://doc.rust-lang.org/std/fs/fn.rename.html)。

用户进一步指出：仍要处理崩溃残片时，是否值得使用 `TempDir`。它的收益主要是正常
控制流上的自动收尾：网络错误、校验失败、解压失败等通过 `?` 提前返回时，无需在
每个分支重复删除目录或自行实现同样的清理守卫。它不会消除异常退出恢复的需要。
当前生产依赖已经包含 `tempfile`，可直接复用；无需为此增加独立清理服务或持久化
下载任务系统。启动时在暂存区回收未被持有的遗留任务即可。

安装流程：

1. 在支持包根目录的暂存区用 `tempdir_in()` 创建本次任务的目录。
2. 将下载中的压缩包写到暂存目录；完整下载后校验预期摘要、大小及包身份。
3. 在暂存目录内解压并检查文件完整性、平台和版本，尚未完成时不向分析器暴露。
4. 检查通过后，在同一文件系统内将完整包整体移动到不可变的正式版本目录，
   完成提交后才标记为可用。避免逐个文件复制进正在使用的版本目录。
5. 普通失败或取消显式清理暂存目录。崩溃或强制退出的遗留目录在下次启动时，
   确认没有任务持有后再清理；不能删除其他活动任务的目录或已安装版本。
6. 如果恰在提交后、通知界面前退出，下次检查应识别已经完整提交的包并复用。

仓库已有应用更新暂存流程使用 `tempdir_in()`、校验、跨进程锁和提交后可见的模式，
可借鉴文件生命周期；语言支持包不需要继承应用更新的重启安装状态机。见
[`application_update/staging.rs`](../../src-tauri/src/application_update/staging.rs)。

依据 Q9，未完成下载不保留用于断点续传；临时文件从不算作已安装的支持包。

## 进度实现约定

目标是一个应用级查询入口和一份共享状态。当前应用更新功能已采用 Rust 权威快照
及 Tauri 查询命令，但前端通过事件订阅接收变化，并不是已有通用轮询器。语言支持包
可以复用“后端维护、前端查询”的边界，按本需求直接实现一个小型轮询 hook。

- 后端负责下载、校验、解压和安装，按包、版本和平台合并重复请求。前端离开页面
  不影响已经开始的共享任务，取消单个 Review 的分析只解除该分析的等待。
- 快照查询只读取本地已维护的状态，不访问 OSS、不检查新版本、不触发下载，也不
  在每次查询时重新遍历安装目录计算体积。
- 前端在应用层挂载一个查询循环，结构页、全局下载提示和设置页共用它的结果。
  活动期间每约 1 秒查询一次，上一请求结束后才调度下一次，避免重叠查询。
- 每个包显示当前阶段：未安装、下载中、校验中、安装中、已就绪、失败。下载阶段
  使用实际已下载字节数和已知总字节数显示进度；总大小未知时显示已下载大小和
  不定进度。校验、解压显示阶段文字，不编造整个流程的百分比或剩余时间。
- 完成或失败状态保留到查询可读、明确重试或删除；不依赖瞬时事件传达结果。
  下载完成只是一个阶段，只有校验和安装提交完成后才可继续结构分析。

接入现有代码的最小方案：

1. 保留现有 `analyze_structure_review` 的长 Promise。后端先识别必要包，等待共享
   管理器准备完成，再执行原分析并返回结果。自动准备不另设前端 prepare/start 命令。
2. 在应用层装配结构分析 adapter 时，记录实际尚未结束的请求数：调用前增加并立即
   查询；Promise 的 `finally` 减少并再查询一次。实际请求尚未结束、包仍在准备或
   最后状态尚未确认时继续轮询，避免首次查询早于后端登记下载而误停。
3. 单个 Review 的取消会立刻改变界面状态，但原 IPC Promise 未必已经结束；轮询
   的请求数以 Promise 生命周期为准。后端取消该分析的等待，共享下载由应用持有。
4. 初次挂载、窗口恢复可见以及管理操作完成后立即查询。只有没有实际请求、没有
   活动包且最后一次查询成功时才停轮询。查询出错显示状态获取失败并继续尝试，
   不能推断下载已结束。完整快照附递增序号，避免管理命令与查询乱序回退状态。
5. 后端在占用 Review Workspace 前，先用固定 Snapshot 的文件元数据识别需要的包；
   等待下载结束后，通过原工作区入口再次校验 Snapshot，才开始分析。准备阶段
   不因等待网络长期占用 Review 工作区。
6. 同一快照提供按分析请求标识关联的必要包列表，由后端使用真实语言与测试过滤
   规则登记，随分析租约释放；准备失败的错误也携带必要包标识。前端不重复按后缀
   猜测，其他 Review 的包错误不能遮住当前工具链或分析错误。

因此自动进度只需要新增一个本地快照查询 command、一个共享 hook，以及后端包准备
逻辑；设置管理按已确认需求增加显式操作。无需通用任务框架、事件总线、外部状态库
或持久化进度存储。

代码依据：[现有分析命令](../../src-tauri/src/tauri_app/commands/structure_review.rs)、
[结构分析 adapter](../../src/infrastructure/tauri-structure-review-adapter.ts)、
[应用层共享控制器](../../src/App.tsx)、
[现有更新快照查询](../../src-tauri/src/tauri_app/commands/application_update.rs)、
[现有更新事件 hook](../../src/application-update/use-application-update.ts)、
[分析工作区入口](../../src-tauri/src/structure_review/application/mod.rs)。

## 验收要求

1. 安装后的应用在目标平台不依赖开发 checkout 的 `.tools`。TS/JS 分析可使用支持包
   中固定版本的 Node 和语言服务；Rust 明确报告所需项目工具链或组件缺失，且不
   隐式安装这些组件。
2. 打开 Review、查看 File Changes 或只分析不支持的文件类型，不启动支持包下载。
   进入 Structure Changes 后，按比较两侧实际分析范围准备必要包；准备完成后自动
   继续原 Snapshot 的分析，失效或取消的分析不得被迟到结果重新激活。
3. 多个 Review 请求相同版本只产生一份下载和安装；下载期间切换页面后仍可在全局
   提示查看状态。混合语言缺少一个包时，不显示不完整的成功结构图。
4. 下载失败、摘要不匹配、解压失败或退出后，都不将暂存内容识别为可用包；普通错误
   及时清理，异常退出残留下次启动回收。重试复用已完成的包，重新下载未完成的包。
5. 设置页显示版本、状态和磁盘占用；未使用的包可以删除，正在下载、安装或被分析
   使用的版本不能删除。用户删去仍被当前应用需要的包后，下次实际需要时重新获取。
6. 轮询只读本地状态，多个界面共享一个轮询器，不因查询重复下载或触发 Review
   更新检查；终态可稳定读到，空闲后停止轮询，查询失败不伪装成下载失败或成功。
7. 前端验收覆盖宽窄窗口、长包名及错误内容、多个包的不同进度、后台提示、设置管理
   与重试；按仓库要求运行 Preview Harness 的真实浏览器检查及适当自动化验证。
8. macOS Apple Silicon 和 Windows x64 均生成可验证、独立编号的支持包产物；使用
   现有 OSS 域名的独立目录分发，应用声明的版本与实际产物一致，配套操作文档能够
   说明如何构建和发布。生产分发可用性在实际上传并读取验证前不能宣称完成。

## 设计确认

Q1–Q11 的产品选择已经回答；本轮补充说明临时目录的收益，以及满足“尽量简单”
要求的进度实现。没有新增产品选择。用户已确认完整约定，并通过 implement 流程授权开始实现。

术语达成一致后即时更新根目录 `GLOSSARY.md`。只有实际选定了有长期成本的架构取舍，
才新增 ADR；其余行为与验收要求记录在本规格中。

## 需要协调的既有规格

- [0019 — Rust Language Analyzer](0019-rust-language-analyzer.md)
- [0020 — Structure Changes Review View](0020-structure-changes-review-view.md)
- [0021 — TypeScript Language Analyzer](0021-typescript-language-analyzer.md)

上述规格已同步：支持包准备属于应用服务，语言分析器只接受已准备好的路径，
TS/JS 不安装项目依赖，Rust 保留既有依赖范围策略，但不得安装工具链或 rust-src。
开发 setup 和注入工具的测试入口保留。

## 实现入口

- `resources/language-support-packs.json` 是应用与打包脚本共用的精确版本表。
- `src-tauri/src/language_support/` 管理共享任务、安装提交、使用租约与清理。
- `src/language-support/use-language-support-packs.ts` 是应用级唯一轮询器。
- `scripts/build-language-support-packs.mjs` 生成四个产物；独立 workflow 原生执行工具，
  发布脚本先验证归档再公开清单，并验证用户访问的 HTTPS 地址。
- [构建和发布操作说明](../language-support-packs.md) 记录首次上线条件、版本升级与回退。

## 实现验收记录

- Rust 全量测试 704 通过、59 个需要显式环境的测试跳过；审查修复后受影响服务、
  工具链和结构分析应用测试 32 通过。额外运行两个真实支持包集成测试均通过，
  覆盖独立 Node 的 TS/JS 分析和直接子目录 Cargo 项目的 Rust 分析。
- 前端全量 47 个文件、424 个测试通过；脚本测试 170 通过；TypeScript 检查、
  ESLint、生产构建及 Rust Clippy 通过。
- 本地生成两个语言包在两个平台的四份归档，并在 macOS arm64 执行真实工具。
  Windows 原生执行由 PR workflow 验证；本地生成 Windows 归档不等于原生运行验收。
- Preview Harness 在 1440×900、760×760、400×760 检查浅色/深色、下载、未知总量、
  安装、长错误、重试和设置删除保护。支持包卡片无横向溢出，长内容可滚动到操作入口；
  验收后恢复默认视口。
- 规范与需求两条独立审查均已完成：存储故障隔离、项目工具链目录、Windows 锁竞争、
  当前请求的包需求和错误契约等发现已修复，复审无剩余发现。

生产分发尚未执行；首次可下载性以受保护发布流程完成公开 HTTPS 回读为准。

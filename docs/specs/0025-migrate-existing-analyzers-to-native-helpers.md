# 将现有语言分析迁移到 Native Helper

状态：实施中，2026-10-11。Q1–Q5 已确认；同日按用户要求调整为本地 macOS 优先、开发完成后集中 CI。本规格不代表新实现已通过验收。

## 目标与范围

将 Rust、TypeScript/JavaScript、Python 三套既有分析实现迁移到统一的 Native Helper
接口，使所有语言分析共用进程通信、生命周期、结果验证及 Code Graph 构造入口。
Java、Go、C#、C/C++ 的既有实现继续使用该公共入口。

本规格承接 [公共图契约](0018-structure-review-code-graph.md)、
[Rust 规格](0019-rust-language-analyzer.md)、
[TypeScript/JavaScript 规格](0021-typescript-language-analyzer.md)、
[Python 规格](0023-python-language-analyzer.md) 和
[Native Helper 公共协议](0024-native-analyzer-helper-protocol.md)。

[ADR-0034](../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 原先明确将既有
分析器迁移留作后续决策。本轮是该后续工作，不能把原先四套实现的验收记录当成本轮迁移证据。
本轮架构决定记录在 [ADR-0035](../adr/0035-migrate-existing-analyzers-to-native-apis.md)，
不改写此前决定的历史范围。

## 已确认的兼容基线

2026-10-11，用户确认“统一架构、保持现有能力”：

- 保留现有 Code Object、Code Object Content 和关系的含义，以及选集与上下文的区别。
- 保留各语言现有支持范围、测试文件开关和已有依赖选项。
- Rust 保留仅仓库、标准库、全部依赖三档；不能以新 helper 默认依赖范围为由删除既有选项。
- Python 保留 3.7–3.14 的目标范围及 3.7–3.9 的专门兼容验收，不借迁移提高版本下限。
- 保留工作区生命周期、取消、失败传播和缺少上下文时的保守关系分类；提取失败不能变成成功空图。
- 迁移中发现的既有缺陷单独说明，不把当前输出自动视为所有语言事实的正确答案，也不静默混入行为调整。

配置语义和规范化文本的兼容精度由下文 Q3、Q4 的明确决定补全。

## 已确认的引擎接入方式

2026-10-11，用户选择直接调用原生 API、移除 LSP：

- Rust helper 内嵌 rust-analyzer 的语法与语义库。
- TS/JS helper 在 Node 内调用 TypeScript Compiler / Language Service API。
- Python helper 用 Rust 内嵌同源 Ruff 与 ty。
- helper 负责语言对象、身份、内容和关系投影，向 Host 返回统一构图数据。
- Host 复用现有通信、进程树管理、来源与图校验、最终 Code Graph 构造。
- 不将现有解析器和语言服务器一并搬入 helper 作为最终实现，也不在原生分析后追加 LSP 阶段。

现有支持范围仍是验收基线。直接 API 的加载策略、绑定结果及资源生命周期须通过专门验证，
不能仅因接口名称相同就视作替换完成。

## 迁移前实现核对

以下保留迁移前只读核对基线。当前三套生产入口已迁到公共 Native Host；本地 macOS
安装与 Review 证据见 [迁移验证](../research/existing-native-helper-migration-validation.md)。
最终双平台 CI 和发布仍待集中门禁。

| 实现 | 应用内职责 | 外部语义工具 | 迁移中的关键工作 |
| --- | --- | --- | --- |
| Rust | `ra_ap_syntax` 解析、对象与内容投影、身份、引用分类 | rust-analyzer，经 LSP 查询定义、模块与调用 | 保留依赖范围；接入语义数据库与项目加载 |
| TypeScript/JavaScript | Oxc 解析、对象与内容投影、身份、引用分类 | TypeScript Language Server / tsserver | 验证内容规范化、导航语义、JSDoc、JSX 与跨项目行为 |
| Python | Ruff 解析、对象与内容投影、身份、绑定及引用校验 | ty，经 LSP 查询定义 | 保留目标版本、标准库隔离与歧义规则；直接内嵌 ty 尚未在本仓库验证 |

三套实现都已有语言投影和真实工具回归用例。真实工具测试多需显式启用；历史通过记录不能
替代新 helper 路径的验证。Python 测试文件中的 `native` 指真实 ty 服务，不代表已经采用
Native Helper 协议。

## 直接 API 路线的可行性核对

2026-10-11 的源码核对确认以下入口存在。它们支持“移除 LSP 桥接”的候选方向，
但还不能证明项目加载行为、投影结果、取消或双平台运行与现有实现等价。该方向已获用户确认。

| 实现 | 候选 helper | 已核实的直接入口 | 必须先验证的差异 |
| --- | --- | --- | --- |
| Rust | Rust 可执行文件，内嵌同一 revision 的 rust-analyzer 库 | `load_workspace`、`AnalysisHost`、`parse`、`goto_definition`、`parent_module`、`outgoing_calls` | 服务器附加的 crate 根处理、多项目加载、测试 cfg、三档依赖范围及加载降级错误 |
| TypeScript/JavaScript | Node 运行的 helper，调用现用 TypeScript API | `Program` / `TypeChecker` 和进程内 `LanguageService` | Oxc 与 TS 的内容规范化差异、定义导航与符号绑定的区别、项目引用与 JSX/JSDoc |
| Python | Rust 可执行文件，内嵌同一 revision 的 Ruff / ty | `ProjectDatabase`、`parsed_module`、`ty_ide::goto_definition` | Ruff 版本对齐、受限文件系统与环境、旧目标标准库、绑定校验与外部符号身份 |

进程内 `LanguageService` 是 API 对象，不意味着启动 tsserver 或保留 LSP。
Helper 的实现语言也不必等于被分析语言；Python helper 可以用 Rust 实现。

Rust 官方允许库消费者，但不承诺 API 稳定性。现用服务器对应的
`9074e9b4c6bc31d7986ccf4e22d18af70e5508da` 源码要求构建 Rust 1.98；如果采用该 revision，
需要独立安排 helper 构建工具链，不混用应用当前 `ra_ap_syntax` 与另一 revision 的语义库。
不能直接使用默认加载配置：构建脚本、过程宏、自动安装和依赖网络范围必须显式保持既有政策。

Python ty `0.0.84` 对应 Ruff 源码 `11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a`。
该版本 `ty_ide`、`ty_project` 是未发布的内部库；锁定源码可以使用这些入口，但需要承担升级适配。
其 Ruff parser 清单标记 `0.0.14`，应用当前使用 `0.0.15`，不能假定 AST 与内容输出可直接替换。
默认项目加载仍可能发现 Python 环境，直接调用 API 也必须保留受控读取和禁止项目执行的边界。

TypeScript 本轮候选基于已有 6.0.3 工具。官方现有 Compiler API 文档适用于 6.0 及以前，
并提示 7.1 将采用不同 API；迁移不顺带承诺新引擎版本支持。

一手依据：

- [rust-analyzer 库稳定性约定](https://rust-analyzer.github.io/book/contributing/architecture.html#stability-guarantees)、
  [固定 revision 的加载入口](https://github.com/rust-lang/rust-analyzer/blob/9074e9b4c6bc31d7986ccf4e22d18af70e5508da/crates/load-cargo/src/lib.rs)、
  [分析入口](https://github.com/rust-lang/rust-analyzer/blob/9074e9b4c6bc31d7986ccf4e22d18af70e5508da/crates/ide/src/lib.rs)。
- [TypeScript Compiler API](https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API)、
  [Language Service API](https://github.com/microsoft/TypeScript/wiki/Using-the-Language-Service-API)。
- [ty 固定源码关系](https://github.com/astral-sh/ty/tree/0.0.84/ruff)、
  [ty 定义查询入口](https://github.com/astral-sh/ruff/blob/11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a/crates/ty_ide/src/goto_definition.rs)、
  [数据库入口](https://github.com/astral-sh/ruff/blob/11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a/crates/ty_project/src/db.rs)、
  [Python 版本范围](https://github.com/astral-sh/ty/blob/0.0.84/docs/python-version.md)。

## 已确认的迁移边界

### Q3：配置语义

用户选择本轮保留既有行为：Rust 按当前有效配置分析，TS/JS 跨项目目标冲突仍归为 Unresolved。
不因协议统一而自动改为新增四套 helper 的 Configuration Union Code Graph 政策。
Python 的跨项目配置冲突规则同样保留；本轮不增加配置枚举或并集能力。

### Q4：规范化文本兼容精度

用户允许新引擎产生等价的打印样式，而保持节点身份、归属、关系、原文和内容变化判定兼容。
不为逐字复刻旧打印结果维护另一套语法模型。存在语义的空白、换行、字面量、JSX 文本、正则、
自动分号插入等差异仍必须被识别；不能用“排版不同”掩盖实质性变化。
新旧比较应分别检查图身份与关系、原文及规范化内容的差异语义，不把打印样式的逐字一致当作唯一判据。

### Q5：迁移切换方式

用户选择先验证 Rust/Python 的关键 API，再按 TS/JS、Rust、Python 逐个完成迁移。
按用户后续确认的验证节奏，每套先通过本地 macOS 的真实 helper、公共 Host、支持包安装和
Review 验证，再在开发分支切换并删除对应旧生产链路，随后继续下一门语言。
新旧实现仅在迁移验证期间临时对照，不增加永久双路选择或失败自动回退。
全部开发、代码审查和本地 macOS 验证完成后，再集中执行最终双平台 CI；届时提醒用户恢复
GitHub Actions 执行额度。Windows 与最终 CI 的证据仍是交付门禁，支持包不在验收前发布，
迁移全部完成前不把整项工作标记为完成。

### 后续工程细化

公共协议需要显式承载既有 Rust 依赖范围及三种语言目标环境，不能丢失选项。
优先通过明确的语言 profile 和平台类型复用现有协议，不另设语言专用通信协议。
若确实需要改动严格消息字段，必须同时定义版本兼容与现有四套 helper 的验证，不暗中放宽协议。

## 公共接入与复杂度约束

- 复用 `native_analyzer` 和 `child_process::ProcessTree`。各 helper 不另造宿主进程守卫、
  子孙进程追踪或平台测试协调机制；取消、超时、异常退出和工作区租约义务继续成立。
- `native_languages::from_pack`、平台 metadata 校验及应用分派必须覆盖迁入语言。
  Rust 仅仓库模式不能因当前四套入口强制要求非空标准库 catalog 而被意外删除。
- Rust 的全部依赖模式仍沿用用户已有网络策略，其他两档离线；禁止 build scripts、过程宏、
  自动安装。TS/JS、Python 继续只使用仓库与既有标准声明，不增加用户依赖或环境发现。
- helper 必须输出完整的文件覆盖、节点来源及外部符号描述；不能只搬旧 `CodeGraph` 序列化，
  或只比较节点数量就认定公共协议接入正确。
- 迁移支持包的入口、控制文件、必需文件表、版本与完整性信息同步更新。不得覆盖已发布版本；
  固定编译器及运行时版本与被分析项目目标版本仍是不同概念。
- 三套迁移结束后，宿主不再编入 `ra_ap_syntax`、Oxc、Ruff 做这些语言的投影，也不继续分发
  三套旧服务器作为分析后备。删除 LSP 前检查 `Position`、取消类型及测试夹具等剩余消费者。
- TS 通用归档校验仍被打包器使用，须保留或迁至中立模块；不能随旧开发安装脚本一起误删。
- 不增加新的用户设置、配置视图、常驻 helper 池或跨 Review 缓存。

## 验收证据

每门语言需同时通过 helper 自身、公共 Host、真实支持包安装和 Review 到 DTO 的路径。
Node 启动、版本查询或 `transpileModule` smoke 不能代替 Code Graph 验收。
真实工具缺失必须使指定验收失败，不能通过跳过测试获得绿灯。

| 范围 | 必须保留/补齐的验收 | 现有输入与断言来源 |
| --- | --- | --- |
| Rust | 三档依赖、测试排除、cfg 声明、crate 根与自定义根、宏/闭包调用、字段类型和图外符号、禁止下载/构建 | `rust_analyzer/tests/{syntax,test_code,test_scope,toolchain,repository_scope,integration}.rs` |
| TS/JS | 自身内容扣除子对象、完整表达式、格式不改差异、有语义文本及稀疏数组、JSDoc/JSX、配置引用无 dist、路径与依赖隔离 | `typescript_analyzer/tests/{syntax,graph_service,real_service}.rs`、`tests/syntax/content.rs`、`tests/{config,workspace}.test.mjs` |
| Python | 3.7–3.14、property/overload、绑定与重绑定、`.py/.pyi`、跨项目冲突、Unicode/换行、污染环境与链接逃逸、测试排除 | `python_analyzer/tests/{graph,native}.rs` |
| 公共接入 | 严格协议、选集与上下文、external 描述、覆盖/来源、失败传播、取消、退出屏障、两侧图及差异 | `native_analyzer/tests/`、`child_process/tests/`、`language_support/tests/`、`application/tests/` |
| 交付 | 支持包真实安装、脱离开发工具运行、明确的新入口与旧入口删除、双平台记录 | `.github/workflows/language-support-packs.yml`、`scripts/verify-native-analyzers.mjs` |

开发期间先取得本地 macOS ARM64 的真实工具证据；全部开发完成后集中补齐最终 CI，
macOS ARM64 与 Windows x64 均须通过后才能交付。公共进程模块承担完整的平台场景，语言层保留接入
及协议行为测试，避免为每门语言复制整套进程协调用例。保留代表性多文件样本的时间/内存记录；
不凭 API 直连推断性能一定提升。遵守仓库 Cargo 全局锁和聚焦验证节奏，不手动触发 quality-gate。

分步实施见 [0025a](0025a-existing-native-helper-implementation-plan.md)。

## 公共接入契约（Task 3）

本节定义迁移 helper 的接入接口，不代表 Rust、TS/JS 或 Python 已完成生产迁移。
继续使用 `narrant.native-analyzer/1`，消息字段、严格重复键检查和 completed DTO 不变。
新 profile 由新 helper 在 ready 中明确宣告；旧四套 helper 的请求与图校验保持原契约。

| profile | 标准声明 catalog | 政策 |
| --- | --- | --- |
| `rust-repository-v1` | 空 | 仅仓库、离线；仍需现有 Cargo/rustc，不要求 rust-src |
| `rust-standard-library-v1` | `rust-standard-library` | 仓库与项目工具链标准库、离线 |
| `rust-all-dependencies-v1` | `rust-standard-library` | 全部依赖，沿用用户网络政策 |
| `typescript-static-v1` | 三个 `typescript-declarations` | 仓库与既有 TypeScript、Node、undici 标准声明 |
| `python-static-v1` | 一个 `python-embedded-stdlib` | 仓库与同源 ty_vendored 内嵌声明，无解释器或环境发现 |

Rust 标准声明平台 root 为已发现的项目 sysroot，metadata 仅含 `rust_src`：
相对该 root 的标准源码目录，并校验 `core/src/lib.rs` 与 `std/src/lib.rs` 存在且不逃逸。
项目工具链发现仍需按本侧 manifest 目录和 rustup 覆盖规则运行；helper 构建版本不是项目目标版本。
Host 为 Rust profile 固定快照 PATH、HOME/USERPROFILE、CARGO_HOME、RUSTUP_HOME、
RUSTUP_TOOLCHAIN 和明确的 Cargo 网络、代理及证书变量；工厂已经解析并批准的值优先于
环境快照，确保项目探针与 helper 使用同一工具链及 PATH。helper 构建版本仅用于构建，
运行时不注入该版本，也不猜测或删除用户/启动器提供的工具链覆盖。编译器包装器与编译参数
不进入继承白名单。
前两档强制 `CARGO_NET_OFFLINE=true`，全部依赖保留用户已有值；始终
`RUSTUP_AUTO_INSTALL=0`、`CARGO_CACHE_RUSTC_INFO=0`。原生 loader 仍负责禁用 build scripts
和过程宏，环境白名单不能代替加载政策。

TS 平台 metadata 仅含 `package`，必须恰好各有一个 `typescript`、`@types/node` 和
`undici-types`；root 分别为实际声明目录，检查 `lib.d.ts` 或 `index.d.ts` 的真实出处。
平台 version 表示声明包版本，目标 JS/TS 配置继续由 helper 从固定侧读取。

Python 平台 root 是受信 helper 安装根，metadata 仅含 40 位十六进制 `source_revision`，
version 为内嵌声明所属引擎版本；平台版本与 Python 3.7–3.14 的项目目标不同。
`stdlib/...` 是 core 的虚拟相对来源，不要求在 root 中存在对应文件，也不引入外部 typeshed SDK。
Python 与 TS profile 允许不同词法选集路径指向同一仓内物理文件；重复词法路径和任何仓外链接
仍失败，Java/Go/C#/C++ 仍拒绝物理别名。Python 的 coverage、origin 和图身份继续使用词法路径，
保留 `.py/.pyi` 和内部目录链接身份。TS 沿用旧链的 canonical 去重：每个词法选集路径单独记录
coverage，按真实文件只生成一份图节点，并把未被测试过滤的词法来源聚合到 `origin.selected_paths`。
TS 测试过滤同时保留输入路径和真实仓库相对路径的既有规则。

公共 Native 应用回调携带既有类型化 Rust 依赖 scope，空语言组仍不启动 helper，
文件级 `include_tests` 仍传给同一次分析请求。生产工厂和支持包版本由各语言后续验收任务启用。
`scripts/build-migrating-native-helper.mjs <language> <staging-directory>` 提供固定源码的暂存
构建入口；Rust/Python 必须已有 `narrant-rust`/`narrant-python` 协议 binary，TS 必须已有
`native-helpers/typescript/main.mjs`。只有 core 的 crate 会明确失败，不生成正式支持包。

已迁移集合由 `scripts/native-migration.mjs` 和应用 `PackId::NATIVE_MIGRATED` 显式记录。
双平台 workflow 的 language 选择仅接受该集合；聚焦运行跳过旧服务器构建矩阵和无关 SDK，
保留所选 helper 自身、公共进程/Host、真实安装、应用接入及图/diff 验收。
`all` 保留最终全语言门禁与 Windows 旧 LSP 生命周期边界；未完成语言不会以空跑取得绿灯。

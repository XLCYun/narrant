# 原生语言支持包接入与本机验收

2026-10-03，配合 [0024 协议](../specs/0024-native-analyzer-helper-protocol.md) 与
[0024a profile](../specs/0024a-native-language-analysis-profiles.md)。以下是实现与本机验证记录，
不表示支持包已经远端发布，也不代替 Windows runner 的真实结果。

## 固定入口

`StructureReview::prepare_analyzers` 从同一个应用支持包服务准备语言包，持有实际 worker 的租约，
通过 `native_languages::from_pack` 为两侧冻结相同 helper、profile 和目标 catalog。
`.java`、`.go`、`.cs` 分别路由，C/C++ 源码和头文件进入同一个实现。缺包和缺目标明确失败；
Rust、TS/JS、Python 保留既有分析实现。

| 包 | 执行工具 | 目标上下文 |
| --- | --- | --- |
| `go` | 固定 Go1.23.8 构建的原生 helper | 包内 Go1.23.8 完整 src、VERSION，运行阶段不启动 go 命令 |
| `java` | 私有 JDK25 + 固定 JDT 依赖闭包 | 独立 JDK17 的 release、jrt-fs.jar、modules；已证明不能以较新 runtime 的标准库替代目标 |
| `csharp` | 私有 .NET10 host/runtime + Roslyn | 独立 Microsoft.NETCore.App.Ref10.0.0/net10.0 reference assemblies |
| `c-cpp` | 原生 LibTooling21.1.8 helper 与私有库 | 独立 Clang resource headers + 精确匹配的系统 SDK；Windows MSVC include 是另一个窄信任根 |

所有新包开发版本为0.1.0，目录中 `published:false` 防止生产入口请求不存在的远端构件，
发布器也排除这些包。已经经过生产校验的本地安装仍然可用。构建不解析待审项目的业务依赖，
不执行待审项目的 Maven、Gradle、MSBuild、CMake 或 Go 构建。

`build-language-support-packs.mjs --id <包> --target <目标> --output <目录>` 会构建自己的 helper，
组装允许的 runtime/目标声明，生成实际文件和归档摘要，并在归档解压后重新检查 helper ready。
产物配置 `native-helpers-config.json` 可直接交给真实 Host 图/diff 集成测试。
Java runtime 的执行权限跨打包/安装保留；Go 标准库清单采用紧凑 JSON，遵守原有2MiB清单上限。

## 本机后端验证

macOS ARM64 上四种真实包均完成构建和解压启动。真实安装测试经过本地 HTTP 服务传输这些归档，
完整执行生产摘要校验、安装、工厂、真实 helper 非空图、租约删除保护，以及使用生产分发源配置
重新打开已安装的未发布包。该测试首次完整通过耗时148.81秒。

对应自动化入口：

- `cargo-agent.sh test --lib structure_review::application::tests`：26 passed、4 ignored。
- `cargo-agent.sh test --lib language_support::`：12 passed、4 ignored。
- `cargo-agent.sh test --lib structure_review::native_languages::tests`：2 passed。
- 原生分组专项4项通过，包含原生失败取消其他 helper 并等待回收的回归。
- `NARRANT_NATIVE_PACKS=<目录> cargo-agent.sh test --lib real_native_packs_install_and_analyze_through_production_factory -- --ignored`：四种真实包完整安装/分析路径。
- `NARRANT_NATIVE_HELPERS_CONFIG=<配置文件> cargo-agent.sh test --test native_helper_graphs -- --ignored`：五种语言模式的图、diff、外部符号、相对源码身份和只读验收，详见 [conformance 记录](native-helper-conformance.md)。

上述 Cargo 命令从 `src-tauri` 调用 `../scripts/cargo-agent.sh`，也可以从根目录提供
`--manifest-path src-tauri/Cargo.toml`。实施期间没有运行完整 Rust 测试，也没有手动触发 quality-gate；
最终全量验证由协调者统一执行。

## 前端真实渲染验收

通用 `pnpm dev:preview` 新增可复用的四包状态和五种原生语言图场景。应用内浏览器分别使用
1440×900 与390×844；同时检查截图、可访问 DOM 和实际元素位置/尺寸，结束后恢复默认视口。
没有为预览改写业务数据。

| 场景 | 实际结果 |
| --- | --- |
| `language-support/native-downloading` | 四个包及进度均可见；窄窗口卡片宽270px，内容没有横向溢出。 |
| `language-support/native-failed` | 长错误在卡片内换行；Java 重试后显示四包下载状态与对应进度。 |
| `language-support/native-management` | 新旧七包名称可读；在用 Rust 禁止删除；删除 Go 后变为未安装状态。 |
| `review-reader/structure-native-languages` | 语言弹层列出 C、C++、C#、Go、Java、Rust；切换 Java/Go/C++ 呈现对应对象。 |
| 窄窗口弹层与长路径 | 语言弹层160×216px完整位于视口；显示选项弹层256px同样未越界，交互状态可切换；长文件名在卡片内截断。 |

390px 视口中的结构图仍沿用详情栏和画布并排的既有布局，画布会缩小，缩放控件可访问；
新增语言名称没有产生横向溢出。类型检查通过；相关前端语言支持/结构视图测试共36项通过。

## 平台验证边界

本机验证使用 macOS SDK27.0。CI 的 macOS ARM64 固定 Xcode16.4/SDK15.5，Windows x64
固定 Windows SDK10.0.26100.0 与对应 MSVC 工具/CRT，并下载校验固定 helper 构建 SDK。
两平台自动执行 helper 测试、Host 协议测试、真实图/diff 与生产安装测试。
Windows 和 SDK15.5 的实际成功状态必须以该 workflow 的真实结果为准，不由本机成功推断。

此前中间快照的结果保留为实现记录；最终修复版本的构件与完整验收结果如下。

## 2026-10-03 最终修复快照的本机验收

最终源码与测试 HEAD 为 `e1b1586`，包含 `ea2f28b` 的 Host 修复。Go 已按 `e1b1586`
重新编译、归档、解压并通过 ready 检查；Java/C# 保留刚从 `ea2f28b` 构建的包，Clang
保留 `fcbf1de` 的已验证包。保留包的归档与 manifest 摘要均未变化。

生产安装 **1 passed、0 failed、0 ignored，127.63秒**；完整图验收
**7 passed、0 failed、0 ignored、0 filtered，8.96秒**。安装覆盖四包的下载校验、
工厂分析、租约保护和重启复用；图验收包含六快照差异与同一仓库四语言合并。

此前 `5e743c8` 的七测试成功和 `ea2f28b` 的中间安装成功保留为历史证据。
构件字节数、SHA-256、源码快照、命令和日志统一记录在
[最终图验收证据](native-helper-conformance.md#2026-10-03-最终修复快照的本机验收)，避免维护两份摘要清单。
上述本地验收只覆盖 macOS ARM64 / SDK27.0。随后真实 CI 的 SDK15.5 全流程也已通过，
Windows x64 仍在验收；独立结果见[跨平台验证记录](native-crossplatform-validation.md)。

## 双目标 C# 支持包增量（2026-10-03）

`85318b7` 的 C# 包同时包含精确 net8.0/8.0.0 与 net10.0/10.0.0 标准引用目录。
真实生产安装用例验证两目录、工厂创建及 net8 分析，1 项通过（127.85 秒）；复用字节未变的
其余三个包。最终测试提交 `df62826` 的公共 Host → CodeGraph → diff 八项全部通过
（10.39 秒），包括旧目标 API 隔离和双 TFM 并集。Node 聚焦 14 项和既有打包脚本 5 项通过。
构件摘要、测试修正与历史失败见[图验收记录](native-helper-conformance.md#双目标增量的最终本机验收2026-10-03)。

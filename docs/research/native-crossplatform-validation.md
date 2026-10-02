# 原生 helper 的双平台验收入口

状态：`974054b` 基线的 macOS ARM64 / SDK15.5 已通过真实 CI 全流程；Windows x64 正在草稿
[PR #200](https://github.com/XLCYun/narrant/pull/200) 中验收。Windows 打包脚本的首跑失败已修复，
尚不能把该步骤通过等同于 Windows helper、安装和图验收全部通过。
后续 `85318b7` 增加 C# 两目标支持、负例和第八项真实图测试；本机安装及八项图验收已通过，
测试修正提交为 `df62826`。该增量仍需取得双平台 CI 结果，不能以此前基线结果代替。

## 已取得的 CI 结果

2026-10-03（北京时间），提交 `9ea5acb` 的
[macOS 原生分析任务](https://github.com/XLCYun/narrant/actions/runs/37045220272/job/110964856515)
完整成功，耗时 9 分 6 秒。使用 Xcode16.4 / SDK15.5，完成四包构建、解包和启动，
Go 原生套件及 Java22 / C#24 项测试通过。Clang 为 22 通过、1 项 Windows SDK 专用测试预期跳过。
Rust Host16、factory2 项通过，
真实四包生产安装用例通过（91.05 秒），公共 CodeGraph/diff 七项通过（12.91 秒），没有跳过。
同轮既有 Python/macOS 任务也通过。

该轮 workflow 整体因 Windows 脚本失败而失败：嵌套运行时的执行标记依赖 POSIX mode，
测试也使用了 Windows 无法直接执行的 shell fixture。`974054b` 按目标平台识别 Windows
`.exe`，保留 macOS 执行位检查，并让 Windows 测试真正执行打包后解出的程序。
[修复后的自动运行](https://github.com/XLCYun/narrant/actions/runs/37046327780) 已通过两个
Windows job 原来失败的 Node 步骤，其余 Windows 验收仍在进行。

`974054b` 的[macOS 原生分析任务](https://github.com/XLCYun/narrant/actions/runs/37046327780/job/110968552316)
随后也完整通过：四包构建、Go 套件、Java22 / C#24、Host16 / factory2 均成功；
Clang 同样为 22 通过、1 项 Windows SDK 专用测试预期跳过。
生产四包安装 1 项（81.42 秒）及公共图/diff 7 项（12.22 秒）通过，这两组零跳过。
这轮结果包含 Windows 打包元数据修复；既有 Python/macOS 任务同样成功。
既有 [Python/Windows 任务](https://github.com/XLCYun/narrant/actions/runs/37046327780/job/110968552230)
也已通过，Rust 分组分别为 20、3、4 项，全部零失败、零跳过。

## 固定构件与隔离边界

`resources/native-toolchains.json` 为 macOS ARM64 与 Windows x64 分别固定官方构件 URL、
版本和摘要。Go 1.23.8、Temurin JDK 25.0.4.1+1（执行工具）、JDK 17.0.20.1+1
（分析目标声明）、LLVM 21.1.8 使用 SHA-256；.NET SDK 10.0.100 使用官方 SHA-512。
另外固定跨架构的 `Microsoft.NETCore.App.Ref` 8.0.0 官方 NuGet 标准引用包，SHA-512 与
不可变 catalog 记录对照；只将 `ref/net8.0` 及许可证放入 C# 支持包，不带入该包的 targets
或 analyzers。其目录与 SDK 自带的 net10.0 引用目录独立，执行 helper 的 runtime 仍为 10.0.0。
JDT 3.47.0 的构建 JAR 闭包另由 `native-helpers/java/dependencies.lock.json` 固定。

`scripts/provision-native-toolchains.mjs` 顺序下载到 runner 临时目录，流式验证摘要后解包，
不运行安装器、不修改全局工具链、不解析业务依赖。安装目录已存在时失败，不盲信旧内容。
归档解包完成后删除下载文件，避免 LLVM 压缩包持续占用额外空间。输出私有工具路径到
`environment.json` 与 Actions 的 `GITHUB_ENV`，没有把被分析仓库加入任何依赖来源。

Windows 使用 LLVM 的 `clang+llvm-21.1.8-x86_64-pc-windows-msvc.tar.xz` 开发归档。
官方说明该归档包含 LLVM 开发所需的额外库，普通安装器面向使用编译器的用户，不能据此
推断安装器拥有 LibTooling 开发库。helper 仍链接 LibTooling，不使用 AST JSON 替代。
来源：[LLVM 21.1.8 官方发布说明](https://github.com/llvm/llvm-project/releases/tag/llvmorg-21.1.8)。

## 目标 SDK 和构建前提

| CI 目标 | 固定声明与构建环境 | 验证边界 |
| --- | --- | --- |
| macOS ARM64 / `macos-15` | Xcode 16.4，macOS SDK 15.5 | 固定 `Xcode_16.4.app` 下 SDK，缺失直接失败；与本机 SDK 27.0 的验收分开记录 |
| Windows x64 / `windows-2022` | VS 2022 Enterprise，MSVC Tools 14.44.35207，CRT 14.44.35211，Windows SDK 10.0.26100.0 | 编译器和 CRT 完整目录版本尚待 Windows PR 首跑核实；目录不匹配必须失败，不自动选新版本 |

macOS runner 的官方目录列出 Xcode 16.4 和 SDK 15.5。Windows Server 2022 runner 目录（镜像 20260927.320.1）列出
VS 2022 17.14 系列、Windows SDK 10.0.26100.0 和 VC runtime 14.44.35211；其 README
未提供默认 MSVC Tools 目录的完整版本，14.44.35207 是本次待 runner 验证的明确构建前提。
当前 `windows-2025` 标签已指向 VS 2026 镜像，所以本工作流明确选择官方仍配备 VS 2022 的
`windows-2022`，没有改写目录猜测或静默回退。映射见 [官方 runner 标签表](https://github.com/actions/runner-images#available-images)。
来源：[macOS runner 镜像](https://github.com/actions/runner-images/blob/main/images/macos/macos-15-arm64-Readme.md)、
[Windows runner 镜像](https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md)。

Windows 分别授权 `Windows Kits/10/Include/10.0.26100.0` 和
`VC/Tools/MSVC/14.44.35207/include`。前者是 `clang-target`，后者以
`clang-system-headers` 和 `additional_system_platform_ids` 显式关联。
不把 Program Files、SystemRoot 或磁盘根作为受信 SDK 根；也不运行 compilation database
中的编译器。构建 helper 的 CMake 使用固定 Visual Studio generator、SDK 和 toolset，
这些是本工具的构建输入。Microsoft 说明命令行构建依赖开发工具环境，不能把分析程序的
环境探测当成配置发现依据。来源：[Microsoft C++ 命令行构建文档](https://learn.microsoft.com/en-us/cpp/build/building-on-the-command-line?view=msvc-170)。

## 必须执行的验收

PR 自动触发 `Language support packs / native-analyzers` 两个 matrix job。每个平台：

1. 运行构建/验收脚本的 Node 测试，安装固定私有开发工具。
2. 为 Go、Java、C#、C/C++ 逐一构建真实支持包，并从归档解包后运行 helper ready 烟测。
3. 运行 Go 原生单元测试及 Java、C#、Clang 的真实 helper Python 测试。
4. 经 `scripts/cargo-agent.sh` 顺序运行 Host 协议/进程生命周期测试、语言 factory 测试，
   以及忽略态的真实四包安装/租约/factory 用例。
5. 执行 `--test native_helper_graphs -- --ignored --nocapture --test-threads=1`，要求
   四个 helper 配置齐全，实际公共 `NativeLanguageAnalyzer → CodeGraph → diff` 覆盖
   Go、Java、C#、C17、C++20、同仓库合并以及 C# net8/net10 隔离与并集，共八项。
   缺少任意 helper 或必要 SDK 都失败，不选择性跳过。

`node scripts/verify-native-analyzers.mjs <产物目录> --build` 是完整入口；省略 `--build`
可复用已完成的本地产物，但仍执行全部 helper 和 Rust 验收。现有 Python job 保留。
新支持包的 artifact 名称独立，未改为远端发布，也没有触发 quality-gate。

所有 Cargo 命令通过现有 wrapper 且串行等待。Windows Git Bash 若缺少 lockf/flock，
现有 wrapper 会警告并直接执行；本 CI job 拥有独立 checkout，且不会并发 Cargo。
这不代表已经改造或验证 Windows 开发机跨 agent 的全局锁。

## 摘要来源

2026-10-03 读取以下官方机器可读元数据，将构件摘要固化到清单；没有为了获取摘要
在本机下载两套大型 LLVM SDK。CI 首次运行时才执行完整下载、摘要验证和解包。

- [Go 全版本下载清单](https://go.dev/dl/?mode=json&include=all)
- [.NET 10 官方发布元数据](https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json)
- [.NET 8.0.0 标准引用包官方不可变目录记录](https://api.nuget.org/v3/catalog0/data/2023.11.14.13.48.02/microsoft.netcore.app.ref.8.0.0.json)
- [LLVM 21.1.8 GitHub release API](https://api.github.com/repos/llvm/llvm-project/releases/tags/llvmorg-21.1.8)
- [Temurin 25.0.4.1+1 release API](https://api.github.com/repos/adoptium/temurin25-binaries/releases/tags/jdk-25.0.4.1%2B1)
- [Temurin 17.0.20.1+1 release API](https://api.github.com/repos/adoptium/temurin17-binaries/releases/tags/jdk-17.0.20.1%2B1)

# 原生 helper 的双平台验收入口

状态：`360cd26` 的 macOS ARM64 / SDK15.5 已通过真实 CI 全流程。Windows x64 已通过
四包构建、启动及四种语言的原生测试，Rust Host 测试中的 Node 夹具启动失败，后续生产安装
和公共图尚未执行；正在[草稿 PR #200](https://github.com/XLCYun/narrant/pull/200) 中修复验证。
本机四包生产安装及八项公共图已通过，含 C# net8/net10 目标隔离；尚不能代替 Windows 结果。

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
Windows job 原来失败的 Node 步骤；后续状态见下文，不将步骤通过等同平台验收通过。

`974054b` 的[macOS 原生分析任务](https://github.com/XLCYun/narrant/actions/runs/37046327780/job/110968552316)
随后也完整通过：四包构建、Go 套件、Java22 / C#24、Host16 / factory2 均成功；
Clang 同样为 22 通过、1 项 Windows SDK 专用测试预期跳过。
生产四包安装 1 项（81.42 秒）及公共图/diff 7 项（12.22 秒）通过，这两组零跳过。
这轮结果包含 Windows 打包元数据修复；既有 Python/macOS 任务同样成功。
既有 [Python/Windows 任务](https://github.com/XLCYun/narrant/actions/runs/37046327780/job/110968552230)
也已通过，Rust 分组分别为 20、3、4 项，全部零失败、零跳过。

最新提交 `802fadf` 的[macOS 原生任务](https://github.com/XLCYun/narrant/actions/runs/37053187055/job/110991346136)
于 2026-10-03 03:25（北京时间）完整成功。六份固定开发构件均下载、校验、解包成功，
包含新 net8 引用包；四个支持包构建成功。真实 Java25 / C#29 项通过，Go 套件成功，
Clang22 项通过、1 项 Windows SDK 专用测试预期跳过；Host16 / factory2 通过。
生产四包安装 1 项（84.24 秒）及公共图/diff 8 项（14.79 秒）通过，两组零跳过。
本轮直接包含 C# 两目标隔离、完整失败及执行哨兵增量，日志保留在
`/private/tmp/narrant-ci-macos-37053187055.log`。既有 Python/macOS 任务也通过。
同轮 [Python/Windows 任务](https://github.com/XLCYun/narrant/actions/runs/37053187055/job/110991346412)
也完整通过，Rust 三组 20/3/4 项均零失败、零跳过；日志为
`/private/tmp/narrant-ci-python-windows-37053187055.log`。这不代替新增四语言的 Windows 验收。

`802fadf` 的任务已覆盖旧基线的全部检查，因此协调者在 03:35（北京时间）取消仅剩旧 Windows
任务的 `37046327780`，停止过时的重复运行；最新 `37053187055` 保持执行。旧任务结果是
**cancelled**，不是测试失败或成功。官方终结日志表明前四种工具迅速进入下一步，LLVM 准备从
02:18 到取消时未结束；旧脚本没有子阶段计时，不能据此判断下载、校验还是解包慢。
最新脚本已有三个阶段的计时，随后取得如下定位结果。

`802fadf` 的 [Windows 原生任务](https://github.com/XLCYun/narrant/actions/runs/37053187055/job/110991346331)
于 05:18（北京时间）到达任务的 120 分钟期限，官方结论为 **cancelled**；本轮没有人为取消。
LLVM 在 03:19:46.939 完成下载、03:19:48.063 完成 SHA-256 校验，之后一直没有解包完成标记，
直到 05:17:58 runner 取消。其余五份构件均已完成下载、校验和解包。
因此问题定位在 Windows `System32/tar.exe` 解包固定 LLVM `.tar.xz` 的阶段；日志尚不能证明
底层是 XZ、libarchive、磁盘或扫描的哪一种原因。该轮未执行 SDK 选择或四语言原生测试。
完整官方日志保留为 `/private/tmp/narrant-ci-native-windows-37053187055.log`。

修复只替换 Windows `.tar.xz` 的提取路径：使用 runner 已有的 7-Zip，以两个直接启动的进程
连接 XZ 解码与 TAR 提取，二进制流传输，不生成巨型中间 TAR。两个进程都成功关闭才算解包
成功；30 分钟独立期限触发后回收进程树，回收未确认则保留临时目录并失败。LLVM 下载源、
摘要、版本和预期目录不变，其他归档格式及 macOS 继续使用原路径。
本机进程生命周期测试不能证明 Windows 解包成功；CI 强制运行真实 7-Zip 小归档测试，
随后仍须完成同一官方 LLVM 归档及完整四语言验收。

修复 `9e164b7` 随 `6d1081a` 推送后，
[Windows 任务 111039380577](https://github.com/XLCYun/narrant/actions/runs/37067660266/job/111039380577)
确认真实 7-Zip 小归档测试通过（306.6883 毫秒），runner 使用 7-Zip 26.03。
完整 LLVM 归档解包成功，耗时 87.0 秒，六份工具链全部准备成功。
任务随后在 SDK 目录检查失败：固定 MSVC Tools 14.44.35207 的 include 和 Windows SDK
10.0.26100.0 的 ucrt/um/shared 均存在，但 `VC/Redist/MSVC/14.44.35211/x64/Microsoft.VC143.CRT`
不存在。该值此前来自 runtime 版本清单，尚需核实其与分发目录版本的映射，不能猜测另一目录。
本轮仍未执行 Windows helper 构建；完整日志为 `/private/tmp/narrant-ci-native-windows-37067660266.log`。
同轮 macOS 原生任务完整成功：Java25、C#29、Clang22 通过及1项 Windows 专用跳过，
Host16、factory2、生产安装1（96.63秒）、公共图8（16.91秒）通过。
既有 Python 双平台任务也成功；这轮 workflow 总体仍因上述 Windows 原生任务而失败。

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
| Windows x64 / `windows-2022` | VS 2022 Enterprise，MSVC Tools 14.44.35207，runtime 版本配置 14.44.35211，Windows SDK 10.0.26100.0 | 编译器、SDK、runtime 元数据及十个私有 CRT DLL 的版本已通过真实检查 |

macOS runner 的官方目录列出 Xcode 16.4 和 SDK 15.5。Windows Server 2022 runner 目录（镜像 20260927.320.1）列出
VS 2022 17.14 系列、Windows SDK 10.0.26100.0 和 VC runtime 14.44.35211；其 README
未提供默认 MSVC Tools 目录的完整版本，14.44.35207 已由本次 runner 的真实目录检查确认。
runtime 清单中的产品版本不能直接当成 Redist 目录名，也不能代替私有 CRT DLL 的版本检查。
微软自身项目通过 `VC/Auxiliary/Build/Microsoft.VCRedistVersion.default.txt` 读取目录标识再定位
CRT；该标识与 Tools 版本独立。[Microsoft Xbox GDK 示例](https://github.com/microsoft/Xbox-GDK-Samples/blob/cdbec9e9b95fdf931b0802fc429fc3719c2efa47/Samples/Tools/CMakeXboxConsoleApp/CMakeGDKXbox.cmake#L65-L70)、
[Microsoft DbgShell 示例](https://github.com/microsoft/DbgShell/blob/7f98155b863c00295cd24513cc2a4c1c0fdf346c/DbgEngWrapper/DbgEngWrapper.vcxproj#L79-L84)。
修复 `8d896f4` 按该元数据选择唯一 CRT 根，并校验目录内全部 DLL 的数值版本前三段是否等于
配置 `14.44.35211`，同时记录完整四段版本。元数据缺失、越界、必需 DLL 缺失或版本不符均失败，
诊断中的目录列表仅用于定位，不用于选择最新版本。本机专项 11 项通过；Windows 版本资源
读取测试为 CI 必跑项，不能用 macOS 的该项跳过代替实际私有 CRT 验证。
后续 `4fa0f0d` 的真实 Windows 任务已确认元数据为 `14.44.35112`，对应 CRT 目录中的十个
DLL 均为 `14.44.35211.0`，目录与版本校验全部通过。LLVM 解包同轮耗时 34.4 秒。
Go、Java、C# 包构建和 ready 烟测已通过；Clang 编译后链接失败，LLVM 导出依赖指向其构建
机器上的 `VS2019/Professional/DIA SDK/lib/amd64/diaguids.lib`，本 runner 使用 VS2022。
目前定点修正该开发库依赖映射；尚未执行后续 Windows 语言套件、Host、安装和公共图测试。
见[真实 Windows 任务](https://github.com/XLCYun/narrant/actions/runs/37069139995/job/111044162856)，
完整日志为 `/private/tmp/narrant-ci-native-windows-37069139995.log`。
`bf8f9d1` 的修复只调整本次 CMake 配置中 `LLVMDebugInfoPDB` 的固定旧 DIA 依赖项，替换为
已指定的 `CMAKE_GENERATOR_INSTANCE` 下对应库；要求库存在且旧项恰好出现一次，保留其他依赖
及顺序，不改写 SDK 文件。真实 CMake 导入目标回归先复现失败，修复后替换、缺库、意外导出
三个用例通过；既有 unittest discover 会在两平台运行它们。实际 Windows 链接仍需后续 CI。
同轮 macOS 原生任务完整通过，生产安装 1 项（86.88秒）、公共图 8 项（13.79秒）均零跳过；
Java25、C#29、Clang22通过加1项Windows专用跳过、Host16/factory2维持成功。
Python Windows 的 Rust 20/3/4 项也全部通过（19.24/2.90/4.93秒），零失败、零跳过。
`0e6f8e8` 的[下一轮 Windows 任务](https://github.com/XLCYun/narrant/actions/runs/37071028213/job/111050242159)
确认 DIA 映射、Clang 链接成功，四包均完成构建及 ready 烟测。第一组 Go 测试随后在
`TestWorkspaceUnion` 失败：fixture 总是追加宿主条目，在 Windows 上得到 host、linux、windows
三个配置，而用例预期两个。`f749fbc` 仅把该 fixture 限定为要验证的 linux/windows 两项，
生产配置发现和并集逻辑未变；三个相关测试在本机通过。后续 Windows Java/C#/Clang 套件、
Host、安装及公共图测试尚未执行。完整日志为 `/private/tmp/narrant-ci-native-windows-37071028213.log`。
同轮 macOS 全流程通过，Clang 加上 CMake 回归后共 25 项通过、1 项 Windows SDK 专用跳过；
Java25、C#29、Host16/factory2、安装1（86.82秒）、图8（14.02秒）通过。Python Windows 20/3/4
也通过（21.26/4.03/6.40秒），零失败、零跳过。
`2e56a6e` 的[Windows 任务 111053944871](https://github.com/XLCYun/narrant/actions/runs/37072185184/job/111053944871)
确认 Go 套件（2.498秒）、Java25（56.172秒）、C#29（20.774秒）全部通过。
Clang26 项中 24 通过、1 项跳过、1 项错误：Unicode 路径测试的 Python stdout 管道默认使用
Windows cp1252，无法解码 helper 的 UTF-8 输出。修复只将该测试驱动的三条管道显式设为严格
UTF-8，不改变 helper 生产协议。Rust Host、factory、安装和八项图尚未执行；完整日志为
`/private/tmp/narrant-ci-native-windows-37072185184.log`。
同轮 macOS 原生任务完整通过（Clang25通过+1跳过、安装1/89.70秒、图8/16.19秒）；既有
Python Windows 的20/3/4项也通过（22.53/4.29/7.33秒），零失败、零跳过。
`360cd26` 的[Windows 任务 111058341797](https://github.com/XLCYun/narrant/actions/runs/37073573624/job/111058341797)
确认严格 UTF-8 测试驱动修复有效：Go（1.817秒）、Java25（44.341秒）、C#29（18.745秒）
成功，Clang26项中25通过、1平台跳过（7.508秒），Unicode 用例通过。
随后首次执行的 Rust Host 分组为6通过、9失败、0跳过（5.61秒）：Node24夹具在协议启动前
发生 `ncrypto::CSPRNG(nullptr, 0)` 断言并以134退出，多个用例因此缺少协议消息或PID文件。
生产 factory 原本显式传入 Windows `SystemRoot`，Node 夹具和公共图直连测试入口遗漏了该项；
修复让这两个测试调用方采用相同的最小环境，保留 Host 的 `env_clear` 及其他注入变量隔离。
新增真实 Session 回归检查 crypto、显式环境、scratch 与进程退出，本机 Host17项通过。
实际 Windows 仍须复验该差异能否消除 CSPRNG 失败；factory、生产安装、公共图未执行。
完整日志为 `/private/tmp/narrant-ci-native-windows-37073573624.log`。
同轮 macOS 原生全流程成功，Host16、factory2、生产安装1（90.57秒）、公共图8（15.55秒）
均零跳过；既有 Python 双平台也成功，Windows 的20/3/4项零失败、零跳过。
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

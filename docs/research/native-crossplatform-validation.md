# 原生 helper 的双平台验收入口

状态：macOS ARM64 / SDK15.5 已通过真实 CI 全流程。Windows x64 已通过四包构建及
四语言原生套件，早期诊断已复现 Job 活跃计数归零但同一进程句柄仍未退出的同步缺口，
正在接入新增 native helper 专用回收实现，既有 LSP/Python 行为不变；后续生产安装和公共图尚未完成。
工作在[草稿 PR #200](https://github.com/XLCYun/narrant/pull/200) 中继续。
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
`d498548` 的[Windows 任务 111066344312](https://github.com/XLCYun/narrant/actions/runs/37076131635/job/111066344312)
再次通过全部四语言原生套件：Go（2.442秒）、Java25（56.780秒）、C#29（21.568秒）、
Clang25通过及1项macOS平台跳过（10.053秒）。Node CSPRNG 启动故障已消除，新增最小环境
回归通过；Host16项中15通过、1失败（3.60秒）：`subprocess_reaps_descendants_after_parent_exit`
预期错误却得到成功。Node/libuv 在 Windows 上默认将普通子进程纳入随父退出清理的内部 Job，
使夹具不能可靠制造遗留后代。修复仅在 Windows 设置 detached 来绕过 Node 的内部 Job，
不请求从 Rust 的外层 Job breakaway；后代自行写入就绪 PID 后父进程才退出，保留两种管道模式
的退出结果和立即回收断言。本机 Host17项通过，Windows 仍需实际复验。
机制依据：[Node24/libuv 子进程实现](https://github.com/nodejs/node/blob/v24.18.0/deps/uv/src/win/process.c#L1052-L1102)。
该轮未运行 factory、安装和公共图。
完整日志为 `/private/tmp/narrant-ci-native-windows-37076131635.log`。
同轮 macOS 全流程通过：Host17（3.82秒）、factory2、安装1（88.61秒）、公共图8（13.86秒）
均零跳过；Python Windows 的20/3/4项也通过（21.67/5.28/9.76秒）。
`d628bfd` 的[Windows 任务 111075220175](https://github.com/XLCYun/narrant/actions/runs/37079022600/job/111075220175)
通过四语言原生套件：Go（1.882秒）、Java25（35.123秒）、C#29（13.003秒）、Clang25通过
及1项macOS平台跳过（6.559秒）。Host16项中14通过、2失败（3.28秒），分别为正常返回及
Drop/panic 后的进程存活断言：`WaitForSingleObject(..., 0)` 返回258而非0。
现有断言只按PID重新打开进程，且错误没有区分父/后代，因此该日志尚不能单独证明后代逃逸
或生产回收失败。后续将保留真实进程句柄、核实准确Job归属并观察退出信号，缩短诊断流程。
该轮 factory、生产安装、公共图仍未执行；完整日志为 `/private/tmp/narrant-ci-native-windows-37079022600.log`。
同轮 macOS 全流程通过（Host17、factory2、安装1/90.83秒、图8/13.61秒，Rust零跳过）；
Python Windows 的20/3/4项通过（21.25/3.39/4.69秒），零失败、零跳过。
诊断改动 `b0ed324` 新增独立 Windows 进程探针，直接引用生产 `WindowsJob`，在 SDK 下载和
Tauri 编译前运行。三个场景记录后代的准确 Job 归属、回收前固定的进程句柄、立即退出信号和
Job 活跃计数；原 Host 测试同步保留父子句柄，原有立即回收断言仍然有效。生产回收算法未改。
本机 Windows 目标编译检查、两项 macOS 生命周期测试和九项 CI 接线测试通过；真实 Windows
行为仍待运行证据，短探针不替代完整安装和公共图验收。
`76f0204` 的[Windows 早期诊断任务 111087221282](https://github.com/XLCYun/narrant/actions/runs/37082946157/job/111087221282)
在探针编译成功后，于 Node 可执行路径检查退出：`NARRANT_TEST_NODE must be an existing absolute executable`。
三个 Job 场景均未执行，不能据此判断进程回收根因；也未进入 SDK、helper、Host、安装及图验收。
本轮日志未打印实际变量值，因此尚不能区分 PowerShell 命令解析与 MSYS 路径转换。
后续诊断入口将沿用已有完整验收的 Node `process.execPath` 与直接子进程环境传递，并输出路径检查证据。
`e1f8985` 的[Windows 早期诊断任务 111089160997](https://github.com/XLCYun/narrant/actions/runs/37083593430/job/111089160997)
已通过 Node 路径入口，并在首个父退出场景复现生产同步问题：父 PID4632、后代 PID7116
均属于准确 Job，清理前活跃计数3。`terminate_and_wait` 返回成功且计数为0，
但清理前固定的后代句柄立即 `WaitForSingleObject(..., 0)` 仍为258，父句柄为0。
因此在此实际场景中，PID复用及Job逃逸均已排除，活跃计数归零不足以证明全部进程对象退出。
完整日志 `/private/tmp/narrant-ci-native-windows-37083593430.log`；仅首场景执行，后续完整验收尚未运行。
上一轮 `76f0204` 的 macOS 已完整通过：Host17（3.61秒）、factory2、安装1（84.14秒）、
图8（14.47秒），Rust零跳过；在替代运行启动后，旧轮剩余 Python Windows 任务被手动取消，
其取消结果不计为通过。
共享生产 `WindowsJob` 的候选修改尚未应用：自动审批要求先验证隔离机制并明确批准其对现有
Windows LSP/Python 的影响。新增探针先独立测试零活跃进程上限的读回、拒绝新建及保留已有进程，
再执行原有三项生产回收硬断言；机制验证通过不会替代回收验收。候选生产补丁另行审查。
`e244845` 的[Windows 门禁诊断任务 111092472010](https://github.com/XLCYun/narrant/actions/runs/37084709435/job/111092472010)
在首个准入门禁场景等待标记10秒后超时；已有日志只记录父子准确Job归属及清理前状态，
没有取得零上限读回、拒绝新建的完整证据，原有三项清理场景尚未执行。
后续仅在独立探针增加设置/读回阶段、同步异常、父子状态与有界错误输出，生产仍未修改。
惰性候选补丁审查另有一项未闭合问题：清理时枚举可能遗漏已经进入退出流程的成员，
因此候选尚不能作为满足完整回收合同的最终修复。
`0671755` 的[Windows 诊断任务 111094268752](https://github.com/XLCYun/narrant/actions/runs/37085313601/job/111094268752)
读回门禁 flags=0x2008、active_limit=0，已有父子句柄仍为258；Node 创建请求同步抛出 UNKNOWN。
该错误尚未包含原始 Win32 错误码，保留为诊断失败，不能据此声称门禁或完整回收已经通过。
同轮 macOS 全流程成功（Host17、factory2、安装1/83.03秒、公共图8/13.99秒，Rust零跳过），
Python Windows 的20/3/4项也通过（22.78/3.41/4.76秒）。
2026-10-03 用户已明确授权必要的共享 Windows LSP/Python 回收修复，保持原 helper 后代能力；
此前审批阻断解除，没有采用单进程限制。新探针先固定原立即 Wait(0) 结果，再观察准确 Job PID列表
与同一句柄的 wait/list/wait，诊断延迟不能掩盖原红灯。现有证据只证明活跃计数不足，尚未实测
证明 PID列表会遗漏未退出对象。新的完整性方案在设计上使用全生命周期保留句柄和累计
TotalProcesses 对账，通知丢失或无法捕获对象必须明确失败；实现与共享回归仍须验证。
`817b3bb` 的[Windows 诊断任务 111152999508](https://github.com/XLCYun/narrant/actions/runs/37105473421/job/111152999508)
取得更直接的退出次序证据：清理前成员列表为[6736]；清理返回后立即保存的后代句柄状态为258，
随后同一次采样是 wait_before=258、members=[]、wait_after=258、exactJob=true、active=0。
下一次采样才变为0。原硬断言仍根据最初258失败，没有用诊断等待把结果改为通过。
因此成员列表为空也不足以证明已经持有的对象完成退出，生命周期跟踪的完整性问题已有实测依据。
这只证明短暂的退出同步窗口，不证明进程持续执行或永久泄漏。日志为
`/private/tmp/narrant-ci-native-windows-37105473421.log`；本轮后续完整原生验收未运行。
共享实现接入再次被自动审批拒绝，理由是影响现有 LSP/Python 且机制尚缺真实验证；用户授权仍有效。
当前先在独立探针验证生命周期跟踪与累计对账，生产清理尚未接入该候选机制。
候选探针已接入三种父子生命周期及真实漏捕获反例：保存准确进程对象，按统一两秒期限
封闭创建、终止、停止监听、等待并核对累计数量；故意漏掉的短命成员必须导致错误且重试仍报错。
独立 Rust 门禁夹具直接检查 raw OS error=1816，保留已有父子存活与准确 Job 归属断言。
本机 Windows 目标编译、Rust 格式和 JS 语法检查通过，实际候选执行尚待 CI。
默认先运行候选，再运行原生产三个场景；生产红灯仍会使整个任务失败，不会被候选通过覆盖。
探针通过握手阻止根进程提前创建后代，不能替代未来生产在 Assign/Resume 前建立跟踪的验证。
候选停止后不再排空通知，漏消息由补捕和累计计数检测；Drop 最终 join 仅为探针避免遗留线程的兜底，
对任意调度/API异常不承诺硬两秒墙钟上限，生产接入仍需独立审查与回归。
同轮其余三项任务全部成功：macOS 原生 Go0.549秒、Java25/17.380秒、C#29/11.330秒、
Clang25通过+1平台跳过、Host17/3.70秒、factory2、安装1/80.42秒、公共图8/15.85秒。
Python Windows20/34.98秒、3/3.78秒、4/4.47秒通过；Python macOS35/3/4项通过，
各 Rust 分组零失败、零跳过。该运行已全部结束，只有原生 Windows 早期探针失败。
`6b18b68` 的[候选 Windows 任务 111156082562](https://github.com/XLCYun/narrant/actions/runs/37106542040/job/111156082562)
已实际通过三种候选生命周期：每次保留3个对象、累计3、活跃0，父子句柄立即为0，耗时3/3/9毫秒。
故意漏掉真实短命成员时保留3、累计4，返回覆盖错误且重试仍报错。Rust 门禁得到准确1816，
已有父子仍为258，验证成功。随后原共享生产场景仍在立即258断言处失败，整个任务保持失败；
候选通过没有覆盖生产红灯。完整日志 `/private/tmp/narrant-ci-native-windows-6b18b68.log`。
在该实测之后，共享接入仍被自动审批拒绝，理由是对所有既有 LSP/Python 会话的影响未充分验证；
被拒绝补丁未应用。当前采用实质更小的范围：只在 `native_analyzer` 内新增专用 Windows 所有者，
迁入单份已验证跟踪器，保持首次 Assign 前附加端口、Resume 前固定根对象；旧 LSP/Python 行为不变。
保留 helper 后代能力，清理时才封闭新成员。代价是有限重复悬挂启动原语；旧共享完成次序缺陷
没有因此修复，既有回归不删除、不放松。专用实现已完成，本机库检查及17项 Host 聚焦测试通过，
新 owner 的 Windows 目标编译通过，实际平台运行与最终差异审查仍待完成。
原生探针已直引新的专用所有者并移除候选副本；生产只维护一份跟踪器。
新增三个 Windows 测试覆盖执行前入组、真实短命成员缺口和终止前失败时的 direct Child 回收，
另一个 ignored 函数仅用作子进程夹具，不能被计作跳过了实际验收用例。
同轮 `6b18b68` macOS 完整通过（Host17、factory2、安装1/83.04秒、公共图8/14.62秒），
Python Windows20/27.80秒、3/3.53秒、4/9.85秒及 macOS35/3/4项通过；所有任务已结束。
`d2b0245` 的[实际专用 owner 任务 111159181765](https://github.com/XLCYun/narrant/actions/runs/37107634743/job/111159181765)
已通过真正产品代码的早期探针：父退出保留管道、父退出关闭管道、父仍存活三场景均在返回后立即
观察父子原 HANDLE 为0，cleanup耗时3/3/7毫秒；raw1816门禁也通过。日志为
`/private/tmp/narrant-ci-native-windows-d2b0245.log`，不再是候选副本结果。
后续 Node 脚本测试为28通过、1失败、1平台跳过；唯一失败是 CI 接线测试使用只匹配LF的正则，
不接受 Windows 检出的 workflow CRLF。生产回收没有再次失败；SDK、共享LSP回归、Host、安装和图
仍未执行，不能提前标记平台完成。当前只修正测试的换行兼容性，所有路径与验收断言保留。
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

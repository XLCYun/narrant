# Windows 自托管 CI

`language-support-packs.yml` 支持通过仓库变量选择 Windows runner。
未设置变量时，继续使用各任务原有的 GitHub 托管镜像；macOS 任务不受影响。
`quality-gate`、发布及桌面更新验收工作流分别管理，不随此变量切换。

## 注册与调度

个人账号的仓库需要分别注册 runner；同一台机器可以复用工具，但每个 runner
必须有自己的安装目录和工作目录。当前先为 `XLCYun/narrant` 注册一个实例，
保留默认的 `self-hosted`、`windows`、`x64` 标签，并添加 `narrant-ci`。

在 runner 在线且工具准备完成后，设置仓库变量：

```bash
gh variable set NARRANT_WINDOWS_RUNNER_LABELS \
  --repo XLCYun/narrant \
  --body '["self-hosted","windows","x64","narrant-ci"]'
```

删除该变量即可让后续运行恢复到原有托管镜像。runner 离线时，指定自托管标签
的任务会排队，不会自动改用托管机器。调度依据是注册范围和标签，不是触发者账号。

## 主机准备

Runner 服务采用独立于交互开发账户的运行身份。服务身份必须能读取工具、写入
自己的 runner/工作目录及工具缓存；不要依赖 Codex 用户缓存里的可执行文件。
使用短工作路径，启用 Windows 长路径支持，并保证接电时不会自动睡眠。

需要由 runner 服务环境提供：

| 工具 | 用途 |
| --- | --- |
| Git 与 Git Bash | Checkout 和 `scripts/cargo-agent.sh` |
| PowerShell 7 | 工作流的 `pwsh` 步骤 |
| Rustup | 安装、运行各任务指定的 Rust 工具链 |
| Python 3 | Java 构建脚本及 Java/C#/C++ 的测试驱动 |
| VS 2022 C++ Build Tools、Windows SDK | Rust MSVC 链接与 C/C++ helper 编译 |
| CMake | C/C++ helper 构建 |
| 7-Zip | 解压 LLVM，脚本使用 `%ProgramFiles%\7-Zip\7z.exe` |

Node、pnpm 和语言声明工具继续由工作流准备。精确 MSVC、SDK 及打包 CRT 版本
见 `resources/native-toolchains.json`；工作流通过 `vswhere` 发现包含该工具集的
VS 2022 实例，支持 Enterprise 和 Build Tools。系统已安装的 VC runtime 不能
代替编译工具或打包所需的 CRT 文件。

## 资源与验证

内存较小的 runner 在自己的服务环境中设置 `CARGO_BUILD_JOBS=1` 和
`CMAKE_BUILD_PARALLEL_LEVEL=1`。一个 runner 同时只接一个 job，但并不会自动
限制 job 内部编译器的并行度。以后增加其他仓库实例时，也需要评估实例间的资源
竞争，不能认为多个实例仍会自动串行。

先手动运行 Language support packs 的 `validation_scope=windows-lifecycle`，
确认 checkout、工具链、后台进程及退出清理可用，再运行完整语言验收。
手动完整验收可用 `platform=windows` 或 `platform=macos` 只调度指定平台；
默认 `platform=all` 和自动 PR 验收仍覆盖两平台。发布必须选择 `platform=all`，
单平台结果不能代替最终双平台验收。Windows 的 VS/SDK/CRT 检查先于大型工具链
下载执行；查询 `vswhere` 时先完整读取输出和退出码，再筛选所需实例。
不得手动触发 `quality-gate`。需要交互桌面的 UI 测试另行验证，不以服务模式下
的后台测试代替。记录实际 Windows 版本和提交 SHA，自托管 Windows 10 的结果
不能标记为 Windows Server 托管镜像的结果。

注册 token 仅在本机注册时使用，不写入仓库、日志或会话。下载和任务联网应在
服务身份下验证；交互用户的本地代理可用，不代表 Windows 服务也能使用它。
首次完整构建需记录磁盘和内存峰值，后续按证据决定缓存及容量策略。

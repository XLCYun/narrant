# Windows 自托管 CI

语言支持包、`quality-gate`、更新冒烟和 Windows 发布打包通过 `NARRANT_WINDOWS_RUNNER_LABELS` 选择自托管
runner；未设置变量时仍使用 `self-hosted`、`Windows`、`X64`、`narrant-ci` 标签。
所有任务共用现有实例；桌面任务所需的登录会话配置见下文。

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

删除该变量后仍使用默认自托管标签。runner 离线时任务排队，不会自动改用付费
托管机器。调度依据是注册范围和标签，不是触发者账号。

## 主机准备

Runner 服务采用独立于交互开发账户的运行身份。服务身份必须能读取工具、写入
自己的 runner/工作目录及工具缓存；不要依赖 Codex 用户缓存里的可执行文件。
当前实例使用专用虚拟账户
`NT SERVICE\actions.runner.XLCYun-narrant.desktop-9roujqj-narrant`，由 Windows
管理账户密码。它与 GitHub 的仓库注册范围分别管理；给此账户授权不会同时给使用
共享 `NETWORK SERVICE` 的其他服务授权。工具目录只需读取/执行权限，runner、
Cargo 和 Rustup 目录需要修改权限。切换服务身份前保存原配置、账户权限和目录
ACL，确认 runner 空闲；切换后验证原注册重新上线，并在 CI 中验证实际能力。
旧身份创建的 CI checkout 可能触发 Git 所有权检查；确认无未提交改动后，将其
移至同级备份，让下一次 checkout 由新身份创建，保留工具缓存和 runner 注册。
使用短工作路径，启用 Windows 长路径支持，并保证接电时不会自动睡眠。
服务身份还必须能创建文件和目录符号链接，语言边界测试会实际创建这些链接。
通过本地安全策略为该账户授予 `Create symbolic links` 后，重启 runner 服务
使新登录令牌生效；不需要重启 Windows。配置时保留已有账户的权限。
工作流在大型下载/编译前运行 `scripts/verify-windows-runner.mjs`，以实际服务身份
验证能力；交互账户下成功不能替代此检查。参见 [Microsoft 权限说明](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-10/security/threat-protection/security-policy-settings/create-symbolic-links)。

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

### 小磁盘 runner 的缓存与清理

自托管 Windows 使用一个本地 sccache，容量上限为 `2G`（2 GiB）。将官方发布的
Windows x64 可执行文件校验后安装到服务账户可读取的工具目录，在该 runner 的
`.env` 中设置以下变量，并把 sccache 所在目录加入该文件已有的 `PATH`：

```text
SCCACHE_CACHE_SIZE=2G
SCCACHE_DIR=C:\ci\cache\sccache
RUSTC_WRAPPER=sccache
CARGO_INCREMENTAL=0
ACTIONS_RUNNER_HOOK_JOB_COMPLETED=C:\ci\hooks\narrant-job-completed.ps1
```

修改 `.env` 后需在 runner 空闲时重启服务。这里的 `CARGO_INCREMENTAL=0` 延续
原 CI 缓存步骤的设置，不改变本地开发的增量编译规则；保留 CI 原有的调试信息、
断言和优化配置。启动检查会读取实际 sccache 服务的统计，确认上限确实为 2 GiB。

自托管任务使用本地 pnpm store 和 sccache，避免完整 target 的压缩副本
和上传、下载开销。下载的依赖、
正在编译的 `target`、工具链和最终产物不计入 sccache 的 2 GiB 上限，磁盘仍需
容纳一次完整构建的峰值。

完成当前任务后，通过 runner 专用的 `ACTIONS_RUNNER_HOOK_JOB_COMPLETED`
回收该 checkout 中明确列出的生成物目录。脚本放在 runner 程序目录之外，仅在
匹配的仓库和工作目录中运行；先校验路径，不沿符号链接或 junction 删除目标。
源码、未提交修改、验收日志、工具链和唯一的 sccache 缓存均须保留。上传与缓存
的 post 步骤应先完成，再执行回收；异常中断后的残留由下次 checkout 或空闲维护
处理。旧工作区备份必须先确认已不再使用，再清理其中可重建的产物。

首次配置后，只运行一次 Windows lifecycle 轻量验证：检查服务身份下的缓存上限、
实际编译与进程测试，以及完成 hook 的清理结果。保留维护前后空闲磁盘和清理清单，
无需为了缓存配置重新运行所有语言的完整验收。

先手动运行 Language support packs 的 `validation_scope=windows-lifecycle`，
确认 checkout、工具链、后台进程及退出清理可用，再运行完整语言验收。
已通过打包、仅需补跑原生验收时，可使用 `validation_scope=native-analyzers`；
该选项保留所选语言的全部 helper、Host、安装和 Review 检查，并跳过独立打包任务。
汇总结果时记录两次运行及各自提交，确认已通过的打包代码未变；默认完整验收和发布仍要求两项任务。
手动完整验收可用 `platform=windows` 或 `platform=macos` 只调度指定平台；
默认 `platform=all` 和自动 PR 验收仍覆盖两平台。发布必须选择 `platform=all`，
单平台结果不能代替最终双平台验收。Windows 的 VS/SDK/CRT 检查先于大型工具链
下载执行；查询 `vswhere` 时先完整读取输出和退出码，再筛选所需实例。
仅做手动验收、不需要下载产物时，可指定 `upload_artifacts=false`，保留全部
构建和测试，只省去 GitHub 产物存储。默认和自动 PR 验收仍上传；`publish=true`
会强制上传，不能通过此选项跳过发布所需产物。Actions 执行预算与产物存储额度
分别管理，自托管 runner 也可能因存储额度不足而上传失败。
不得手动触发 `quality-gate`。记录实际 Windows 版本和提交 SHA，自托管 Windows 10 的结果
不能标记为 Windows Server 托管镜像的结果。

## 桌面与更新验收

真实 Tauri E2E 和更新冒烟共用 `desktop-9roujqj-narrant` 及上面的
`NARRANT_WINDOWS_RUNNER_LABELS`。不新增虚拟机、独立系统或另一份 runner 注册。
当前虚拟服务账户运行在 Session 0，不能提供桌面会话；调度修改不会自动改变主机上的
启动方式，准备完成前桌面检查会明确失败。

迁移时先确认 runner 空闲并保存服务配置与目录 ACL，停止原服务，再从已登录的用户
桌面运行原安装目录的 `run.cmd`；可配置仅在该用户已登录时启动的计划任务。
虚拟服务账户不能交互登录，需要选择本机登录账户并授予原工作区、工具缓存的必要权限。
保留原注册、标签、工具配置和完成钩子，不要同时启动服务和桌面两个 Listener。
`scripts/verify-desktop-runner.mjs` 会拒绝 Session 0，再由真实 E2E 验证应用行为。

更新冒烟还需要写系统 hosts、安装用户证书和应用的权限，不要求额外的隔离系统声明。
保留原 hosts 备份、临时证书删除、进程退出及恢复校验；失败时保留恢复所需文件。
每台机器只运行一个 runner，桌面任务依次执行。测试期间避免同时手动运行 Narrant；
若所选登录账户已有 Narrant 数据，现有保护会拒绝冒烟，不得删除个人数据来让测试通过。
先解决登录账户、测试数据目录和系统权限，再运行真实更新验收。

注册 token 仅在本机注册时使用，不写入仓库、日志或会话。下载和任务联网应在
服务身份下验证；交互用户的本地代理可用，不代表 Windows 服务也能使用它。
首次完整构建需记录磁盘和内存峰值，后续按证据决定缓存及容量策略。

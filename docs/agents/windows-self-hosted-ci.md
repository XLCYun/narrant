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

runner 使用当前已登录的 `win10` 桌面账户，独立工作目录为 `C:\ci\runners\narrant`。
工具用户配置也必须放在 CI 专属目录；runner `.env` 设置如下，真实 `USERPROFILE` 保持不变：

```text
HOME=C:\ci\home
GIT_CONFIG_GLOBAL=C:\ci\home\.gitconfig
PNPM_HOME=C:\ci\home\pnpm
NPM_CONFIG_USERCONFIG=C:\ci\home\.npmrc
```

配置文件可为空，不复制日常 Git/npm 配置或凭据。预检会确认两份用户配置位于独立 HOME，
防止后续工作流的 `git config --global` 写入用户日常配置。`pnpm/action-setup` 会将
`PNPM_HOME` 改为自己的专用安装目录；它不作为 Git/npm 用户配置隔离的判断依据。
工具目录只需读取/执行权限，runner、Cargo、Rustup 和缓存目录需要修改权限；
不要依赖 Codex 用户缓存里的可执行文件。原虚拟服务账户
`NT SERVICE\actions.runner.XLCYun-narrant.desktop-9roujqj-narrant` 的服务配置保留用于回滚，
切换前保存原启动类型、目录 ACL 和非敏感环境配置，确认 runner 空闲；
切换后验证同一注册重新上线，并在 CI 中验证实际能力。
旧身份创建的 CI checkout 可能触发 Git 所有权检查；确认无未提交改动后，将其
移至同级备份，让下一次 checkout 由新身份创建，保留工具缓存和 runner 注册。
使用短工作路径，启用 Windows 长路径支持，并保证接电时不会自动睡眠。
runner 的实际令牌必须能创建文件和目录符号链接，语言边界测试会实际创建这些链接。
现有普通桌面令牌缺少该权限时，可使用同一会话中的提升权限计划任务；
不为 CI 新建登录会话、启用自动登录或全局 Developer Mode。配置时保留已有账户的权限。
工作流在大型下载/编译前运行 `scripts/verify-windows-runner.mjs`，以实际 runner 身份
验证能力；其他终端下成功不能替代此检查。参见 [Microsoft 权限说明](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-10/security/threat-protection/security-policy-settings/create-symbolic-links)。

需要由 runner 环境提供：

| 工具 | 用途 |
| --- | --- |
| Git 与 Git Bash | Checkout 和 `scripts/cargo-agent.sh` |
| PowerShell 7 | 工作流的 `pwsh` 步骤 |
| Rustup | 安装、运行各任务指定的 Rust 工具链 |
| Python 3 | Java 构建脚本及 Java/C#/C++ 的测试驱动 |
| VS 2022 C++ Build Tools、Windows SDK | Rust MSVC 链接与 C/C++ helper 编译 |
| 7-Zip | 解压 LLVM，脚本使用 `%ProgramFiles%\7-Zip\7z.exe` |

Node、pnpm 和语言声明工具继续由工作流准备。C/C++ 验收所需的 CMake 由
`provision-native-toolchains.mjs` 下载、校验并解包到任务临时目录，通过 `NARRANT_CMAKE`
传给构建与测试，不要求全局安装，也不进入用户支持包。精确 MSVC、SDK 及打包 CRT 版本
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

修改 `.env` 后需在 runner 空闲时重启当前 Listener。这里的 `CARGO_INCREMENTAL=0` 延续
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

真实 Tauri E2E 共用 `desktop-9roujqj-narrant` 及上面的 `NARRANT_WINDOWS_RUNNER_LABELS`。
原虚拟服务账户运行在 Session 0，不能提供桌面会话；已选择复用当前 `win10` 登录会话，
2026-10-15 已完成原地迁移，原注册 ID 21 保留。实际 Listener 为 `win10 / Session 1`，
原服务已停止并改为手动启动。计划任务名称为 `Narrant CI Interactive Runner`，使用
`InteractiveToken` 和提升权限；文件、目录符号链接及 Git 所有权检查通过。
迁移、核验和回滚记录位于 `C:\Users\win10\ci-setup\interactive-runner-migration-20261015`。

手动启停前确认 runner 空闲；计划任务只会使用已经登录的 `win10` 会话：

```powershell
Start-ScheduledTask -TaskName 'Narrant CI Interactive Runner'
Stop-ScheduledTask -TaskName 'Narrant CI Interactive Runner'
```

迁移时先确认 runner 空闲并保存服务配置与目录 ACL，停止原服务，再从已登录的用户
桌面运行原安装目录的 `run.cmd`；计划任务使用 `InteractiveToken`，仅在该用户已登录时启动。
不保存密码、不自动登录，不新增账户、登录会话或另一份 runner 注册。
保留原注册、标签、工具配置和完成钩子，不要同时启动服务和桌面两个 Listener。
`scripts/verify-desktop-runner.mjs` 会拒绝 Session 0，再由真实 E2E 验证应用行为。

E2E 使用独立应用身份 `com.cyunlab.narrant.e2e`、无痕 WebView、临时数据库和仓库，
凭据只保存在内存，并禁用正式应用更新。测试不运行安装程序，不修改日常应用的安装记录；
不能关闭或清理用户正在使用的 Narrant。测试构建漏传 E2E 配置时会在初始化用户目录前停止。

升级验收使用独立 `Narrant Upgrade Test`，identifier 为 `com.cyunlab.narrant.upgrade-test`，
可执行文件名为 `narrant-upgrade-test`。旧、新测试包分别取上一 Stable 和候选源码，仅叠加隔离身份、
内存凭据和数据观察入口。TLS 通过测试进程自己的代理与 CA 文件工作，不修改 hosts、系统信任或代理。
正式包另做静态签名、完整性与信任根核验；测试包升级不等同于正式产品身份下实际升级。

`update-smoke.yml` 的 `isolated-upgrade` 产生发布验收，`preflight` 只产生不可发布的分支验收。
继续保留 `NARRANT_CI_SHARED_DESKTOP=1` 和 checkout 外 hook 对旧 `native-smoke` job 的阻止，
防止重跑历史工作流误用日常安装。新入口使用 `verify-desktop-runner.mjs --isolated-update-test`，
不要求 sudo、管理员或 hosts 写权限。缺少独立 profile 或发现旧测试安装/数据时停止，保留现场。
失败后只清理本次拥有的测试进程、安装和数据，不能清理日常 Narrant。

注册 token 仅在本机注册时使用，不写入仓库、日志或会话。下载和任务联网应在
服务身份下验证；交互用户的本地代理可用，不代表 Windows 服务也能使用它。
首次完整构建需记录磁盘和内存峰值，后续按证据决定缓存及容量策略。

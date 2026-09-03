# Application Updates

## Problem Statement

Narrant 的公开 Release 目前只提供 DMG、NSIS 安装包和校验和。已经安装的应用既不会
发现新版本，也不能下载、暂存或安全安装新版本。发布流程虽然能从私有源码仓库向公开
`cyunlab/narrant` 仓库写入 Draft Release，但没有 Tauri updater 制品、签名、静态
manifest、真实原生升级验收或受保护的 Stable promotion。

Narrant 还同时持有 Review Workspace、Git 操作和 Babysit provider 进程。应用更新不能
绕过既有退出清理，也不能在清理超时后冒险替换正在使用的程序文件。

## Solution

建立独立的 **Application Update** 能力。客户端从公开仓库的已发布 Release 读取 Stable
manifest，启动时及每六小时检查一次，默认自动下载并把经过 minisign 验证的目标平台包
持久化暂存。用户可以继续工作、稍后正常退出安装，或显式选择“立即重启并更新”。

普通退出与更新重启共用一个幂等应用停止边界：拒绝新操作、取消全部已注册操作并等待
drain，再关闭 Babysit provider、Review runtime/Workspace 和 telemetry。整个停止流程共享
十五秒 deadline；超时则保留暂存包、跳过安装并直接退出。

私有源码仓库继续拥有版本、构建、签名和验证。完整 Draft Release 写入公开
`cyunlab/narrant`；维护者审批私有仓库受保护的 promotion job 后，自动化二次复核 Draft
身份、签名、manifest 和原生升级证据，再将其发布为 Latest。公开仓库不成为源码镜像，
也不要求启用 GitHub immutable releases。

## User Stories

1. 作为已安装 Narrant 的用户，我希望应用自动发现 Stable 新版本，以便不必反复查看下载页。
2. 作为正在 Review 的用户，我希望更新不会在工作中强制关闭应用，以免中断阅读或分析。
3. 作为希望保持最新版的用户，我希望默认自动下载并验证更新，以便退出时即可安装。
4. 作为希望控制网络使用的用户，我希望关闭自动下载但保留自动检查和更新提示。
5. 作为准备立即升级的用户，我希望查看版本与 Release Notes 后选择重启更新。
6. 作为正常退出的用户，我希望已暂存更新在安全清理完成后自动安装。
7. 作为退出清理超时的用户，我希望应用仍能退出，并在以后重试安装，而不是卡住或冒险替换文件。
8. 作为使用辅助技术的用户，我希望更新入口、状态、进度、错误和操作具有清晰名称与焦点顺序。
9. 作为维护者，我希望只有私有源码 tag 能请求发布，并且公开 Release 不暴露私有源码身份。
10. 作为维护者，我希望 Draft 在发布前完成两平台签名、manifest 和真实安装/升级验证。
11. 作为现有 v1.1.5 用户，我接受首个 updater-capable 版本需要手动安装一次；之后版本自动升级。

## Product Behaviour

### Update policy

- 应用启动完成后触发一次检查，此后每六小时触发一次；并发检查合并为一次运行。
- 自动下载首次默认开启并由 Application Update 自己持久化；关闭后仍自动检查。
- 用户可从设置页手动检查。手动检查与自动检查使用同一状态机，不创建第二套流程。
- 只接受 SemVer 严格高于当前版本的 Stable Release；不得降级或重复安装当前版本。
- 网络、408、429 和 5xx 失败最多自动重试三次，退避一秒、两秒、四秒；验证、磁盘和
  manifest 契约错误不可自动重试。用户可以显式重试最近一次可重试操作。

### Observable states

Application Update 对前端发布有序快照，状态至少包含：

- `idle`
- `checking`
- `up_to_date`
- `available`
- `downloading`
- `staged`
- `failed`

快照包含单调递增 sequence、当前版本、候选版本、Release Notes、自动下载偏好、已下载
字节、可选总字节、稳定错误类别和 retryable 标志。前端忽略旧 sequence，不从多个局部
状态推导 updater 真相。

### User interface

- Application Settings 新增“应用更新”分区，展示当前版本、自动下载开关、当前状态、
  手动检查入口和最近错误。
- 当状态为 `available`、`downloading`、`failed` 或 `staged` 时，在侧栏 GitHub Identity
  右侧显示一个小型圆形 **UI Icon**；其可访问名称反映当前状态。
- 点击圆形图标只打开应用内更新弹层，不直接重启。弹层展示候选版本、Release Notes、
  确定或不确定下载进度、可重试错误，以及 `staged` 状态下的“稍后”和“立即重启并更新”。
- `checking` 与 `up_to_date` 不显示侧栏图标；它们仍在设置页中可见。
- 更新弹层不得接受任意 URL、文件路径或签名；所有信任输入由 Rust 能力持有。

## Runtime Design

### Application stop

- 抽出一个 Tauri-managed、幂等的应用停止边界，供普通退出和 Update Restart 共用。
- 第一次停止请求原子关闭 `CancellableOperations` 的注册入口，向所有 Running operation
  广播取消，并等待 RAII registration 归零。后续停止请求复用同一个 outcome。
- drain 后依次调用 `BabysitApplication::shutdown`、`ReviewLifecycle::shutdown` 和
  `Telemetry::shutdown`。Application Update 不包含 provider-specific 清理。
- `ExitRequested`/主窗口关闭先阻止默认退出并异步执行停止；最终内部 exit/restart 放行。
  随后的 `Exit` 事件不得重复清理。
- 十五秒是 cancel、drain 和全部现有清理共享的总 deadline。deadline 到期后普通退出仍
  继续，但本次不得安装；暂存包保留供以后重试。

### Update trust and staging

- 正式 endpoint 固定为
  `https://updates.cyunlab.com/narrant/channels/stable/latest.json`，只允许 HTTPS、固定 host 与 exact path。
- 使用 Narrant 独立的 Tauri updater minisign 密钥。公钥编译进应用；私钥、密码及离线
  备份不进入源码、公开仓库或 Release。
- Rust 同时把 endpoint 与公钥提供给 updater plugin 和 Application Update adapter；缺失、
  非 HTTPS 或无效配置 fail closed，并写入不含签名、凭据或用户路径的诊断。
- 下载流写入临时文件并报告进度；完成后 sync、minisign 验签，再原子替换持久化暂存。
- 暂存目录只保留当前候选包和描述目标、版本、签名、安装类型的 metadata。恢复、安装前
  和进程重启后都再次验证 metadata 与包签名。
- 更高候选替换旧暂存；启动版本达到暂存版本后清理；损坏或不匹配暂存被安全丢弃并报告。

### Platform installation

- 初始目标只有 `darwin-aarch64` 和 `windows-x86_64`。
- macOS updater 包是最终 Developer ID 签名、公证后的 `.app` 重新打包成 `.app.tar.gz`
  并使用 updater key 签名。安装使用可回滚目录替换；权限不足时可以请求系统管理员授权。
- Windows updater 包是 current-user NSIS `.exe` 及其 updater 签名。安装使用 update 参数；
  显式 Update Restart 允许 installer 接管 relaunch，普通退出不强制重启。
- updater 签名与 macOS Developer ID/Windows Authenticode 是独立信任。Windows 首期仍可
  没有 Authenticode，但 updater 签名不可省略。

## Release Design

### Draft assets

每个完整公开 Draft 至少包含：

- `Narrant_VERSION_aarch64.dmg`
- `Narrant_VERSION_x64-setup.exe`
- `narrant-darwin-aarch64-updater.app.tar.gz`
- `narrant-darwin-aarch64-updater.app.tar.gz.sig`
- `narrant-windows-x86_64-updater.exe`
- `narrant-windows-x86_64-updater.exe.sig`
- `latest.json`
- `SHA256SUMS`

`latest.json` 只包含 `darwin-aarch64` 和 `windows-x86_64`。每项 `url` 是 content-addressed OSS
updater package，`installer_url` 是同版本同 target 的 OSS DMG/EXE；`signature` 是对应 `.sig`
文件的字面内容。Draft 阶段
没有最终 published time，因此 `pub_date` 可以省略；Notes 来自公开 Release body。

### Cross-repository authority

- 私有源码仓库的稳定 `vMAJOR.MINOR.PATCH` tag 和完整 commit 是发布请求与版本真相。
- 公开 Release tag 只标识制品，不代表私有源码 commit；验证证据必须分别绑定 source
  tag/SHA 与 public Release ID/tag。
- `CYUNLAB_RELEASE_TOKEN` 继续只对 `cyunlab/narrant` 提供所需 Contents 写权限。
- updater 私钥与密码只存在私有源码仓库受保护环境；公开仓库只获得公钥、包签名和 manifest。
- Draft 可在失败重试中重建资产；已发布 Release 不得被自动化修改。仓库管理员仍保留
  GitHub 原生修改能力，因为本功能不启用 immutable releases。

### Native smoke and promotion

- Draft 上传完成后必须经 API 回读精确资产集、asset ID/digest/bytes，并校验版本、URL、
  manifest signature literal、SHA256SUMS 和 minisign。
- 在 `macos-15` Apple Silicon 与 Windows x64 runner 上执行真实原生生命周期：安装上一
  updater-capable Stable、检查候选、下载、持久化暂存、正常退出安装、原路径替换和候选启动。
- Draft 资产不是匿名公开。smoke 使用临时 TLS gate 向真实旧应用的固定 GitHub endpoint
  提供从 Draft API 回读的 byte-exact manifest 与包；不得降低生产 HTTPS 限制。
- 每个平台生成 checksummed evidence，绑定 source tag/SHA、public Release ID/tag、manifest
  SHA、上一 Stable manifest SHA、包/签名 SHA、runner OS/arch、freshness 和 real-native checkpoints。
- 私有源码仓库中的 promotion job 使用受保护 Environment 等待人工审批。审批后再次回读
  Draft 并复核全部 evidence，才把 Draft 设置为 published/latest。
- 首个 updater-capable Release 没有可自动升级的上一 Stable：它执行完整安装、启动、配置、
  签名和暂存恢复 smoke，并明确作为一次性手动 bootstrap。下一版开始强制旧版到候选升级。

## Implementation Decisions

- Rust 按 capability deep module 组织 Application Update；核心状态机、暂存、平台 adapter、
  Tauri adapter 与退出组合保持窄接口。
- 前端只依赖 provider-neutral update snapshot 与命令，不直接使用 updater plugin JavaScript API。
- Tauri transport 继续使用统一 typed command contract；更新事件使用单一 snapshot event，
  命令只提供读取快照、手动检查、更新自动下载偏好、重试和请求 Update Restart。
- Application Update 的自动下载偏好属于该能力，不把运行时状态塞入通用 `settings.json`。
- `bundle.createUpdaterArtifacts` 为 `true`；生产构建必须提供 updater private key/password、
  endpoint 和 public key。开发与测试使用显式 fixture，不能把空信任配置伪装成可用 updater。
- 新增或修改的方法、结构体和类提供易读中文注释；Rust 私有模块测试放入模块 `tests/`
  目录，通过生产模块中的 `#[path = "..."]` 声明。

## Acceptance Criteria

1. 启动检查、六小时周期、手动检查、并发去重、防降级和默认自动下载行为有确定性测试。
2. 自动下载偏好跨进程保留；关闭后继续检查但不下载，重新开启后可下载当前候选。
3. 下载进度、三次可重试退避、不可重试验证错误和用户重试由状态机测试覆盖。
4. 暂存的原子替换、恢复、篡改拒绝、安装前复验、更高候选替换和新版本清理由磁盘测试覆盖。
5. 普通退出与 Update Restart 只执行一次共享停止流程；十五秒超时跳过安装并允许退出。
6. cancel admission 关闭后拒绝新操作；drain 等待已注册 future 的 RAII guard 释放。
7. Codex 常驻 App Server、Claude fresh process 和 Built-in session 都通过既有 Babysit shutdown
   契约收口，Application Update 不出现 provider-specific 分支。
8. macOS 与 Windows 平台 adapter 覆盖参数、替换、回滚、权限/installer 失败和 staging 清理。
9. 设置页、侧栏圆形图标和更新弹层覆盖全部可见状态、键盘操作、可访问名称与错误重试。
10. 宽窄窗口、长短 Release Notes、确定/不确定进度、失败和 staged 弹层通过 Preview Harness
    与应用内浏览器的截图、可访问 DOM、实际尺寸和交互检查。
11. Draft 生成脚本对精确资产、manifest、签名、tag-specific URL 和公开/私有身份映射有测试。
12. promotion 在缺少、过期、目标不全、来源不匹配或非 real-native evidence 时 fail closed。
13. 首个 updater-capable Release 执行 bootstrap smoke；后续 Release 在两平台执行真实旧版到候选升级。
14. 发布运维文档说明密钥备份、泄露/丢失、bridge rotation、Draft 审批、失败恢复和首版 bootstrap。
15. 交付验证包含前端单元测试、类型检查、Lint、生产构建、`git diff --check`、聚焦 Cargo
    检查/测试，以及 Issue 完成前通过 `scripts/cargo-agent.sh` 运行一次 Rust 全量测试。

## Out of Scope

- Linux、Intel macOS、ARM Windows 或移动平台更新。
- beta/nightly/灰度 channel、强制更新、静默强制重启或自动降级。
- OSS/S3 Stable pointer、CDN、原子对象存储 promotion lock 或动态更新服务。
- Windows Authenticode、Microsoft Store、Mac App Store 或其他商店更新机制。
- GitHub immutable releases。
- 让已安装且没有 updater 代码的 v1.1.5 自动升级。
- 在前端接受自定义 endpoint、公钥、包路径或 Release channel。

## Further Notes

- [ADR-0026](../adr/0026-publish-binary-releases-from-source-tags.md) 约束双仓库发布权威。
- [ADR-0028](../adr/0028-deliver-staged-application-updates-from-public-releases.md) 记录 Stable
  channel、完整暂存生命周期和退出边界的架构选择。
- [Application Settings](./0014-application-settings.md) 提供设置目的地和响应式验收先例。
- [Window State Restoration](./0007-window-state-restoration.md) 约束正常 exit/restart 的窗口状态保存。

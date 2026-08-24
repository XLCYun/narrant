# Application Settings

## Problem Statement

Narrant 没有统一的 **Application Settings** 入口或持久化边界。主题只能跟随系统，
GitHub Identity 的退出只能从侧栏快捷菜单触发，宝宝模式的 Codex 可执行文件只能在启动
失败后临时选择，模型与思考强度则固定在实现中。这些行为分散在前端本地存储、GitHub
生命周期和 Babysit provider 内，无法形成可发现、可验证、可扩展的桌面应用设置体验。

## Solution

在侧栏底部 GitHub Identity 右侧增加圆形设置按钮，进入带左侧二级导航的独立设置页面。
首期提供外观、GitHub 和宝宝模式三个分区。非敏感设备级偏好统一写入应用数据目录的
`settings.json`；GitHub 凭据继续存入操作系统凭据存储。

外观支持跟随系统、浅色和深色。GitHub 分区展示当前 GitHub Identity 并提供带确认的
退出操作。宝宝模式允许显式选择 Codex 可执行文件，并从 Codex App Server 动态读取模型
及其支持的思考强度。显式选择始终受到尊重；失效配置产生可操作错误，不静默回退。

## User Stories

1. 作为 Narrant 用户，我希望在 GitHub Identity 旁看到独立的设置按钮，以便从任何主要页面进入设置。
2. 作为键盘或辅助技术用户，我希望设置按钮具有明确的“设置”名称和焦点状态，以便可靠操作。
3. 作为未来会使用更多设置的用户，我希望设置页采用可扩展的二级导航，以便快速定位不同分区。
4. 作为从 Review 进入设置的用户，我希望离开设置后回到进入前的页面，以便不中断阅读上下文。
5. 作为桌面用户，我希望设置在当前设备和操作系统用户范围内生效，以便不同 Repository 共享一致偏好。
6. 作为升级用户，我希望现有宝宝模式路径设置迁移到统一配置，以便升级后无需重新配置。
7. 作为安全敏感用户，我希望 GitHub 凭据不写入普通 JSON，以便仍由操作系统安全存储保护。
8. 作为偏好系统外观的用户，我希望主题默认跟随系统，并在系统主题变化时同步更新。
9. 作为偏好稳定浅色界面的用户，我希望选择浅色后不再受系统主题变化影响。
10. 作为偏好稳定深色界面的用户，我希望选择深色后应用框架、Monaco 和品牌图像保持一致。
11. 作为调整主题的用户，我希望选择立即生效并持久化，以便即时确认效果。
12. 作为遇到持久化错误的用户，我希望主题恢复到原值并看到错误，以免界面与配置不一致。
13. 作为已登录 GitHub 的用户，我希望在设置页看到当前 GitHub Identity，以便确认授权对象。
14. 作为准备退出的用户，我希望确认框说明会删除什么、不会删除什么，以便放心决策。
15. 作为退出失败的用户，我希望当前 GitHub Identity 保持有效并可重试，以免出现虚假退出状态。
16. 作为高频用户，我希望侧栏身份菜单仍可快捷退出，以便不必每次进入设置。
17. 作为安装多个 Codex 的用户，我希望显式选择一个可执行文件，以便宝宝模式始终使用我的决定。
18. 作为选择 Codex 的用户，我希望使用系统文件选择器并在保存前验证版本，以免保存错误路径。
19. 作为显式配置 Codex 的用户，我希望路径失效时得到错误而不是自动换用另一安装，以便运行时与设置一致。
20. 作为希望恢复默认行为的用户，我希望清空显式路径并恢复自动发现。
21. 作为 Review Reader 用户，我希望 Codex 配置只在设置页管理，以免错误状态中出现第二套选择语义。
22. 作为无需调优的用户，我希望宝宝模式默认自动使用 App Server 的默认模型和默认思考强度。
23. 作为希望控制质量与速度的用户，我希望从 App Server 当前支持的模型和思考强度中选择。
24. 作为查看设置的用户，我希望页面平时只展示保存值，而不是仅为展示就启动 App Server。
25. 作为准备修改模型的用户，我希望点击“修改”后才侦测当前可用模型，以便列表保持新鲜。
26. 作为尚未决定修改的用户，我希望可以取消编辑并完整保留旧配置。
27. 作为保存模型的用户，我希望保存前验证模型与思考强度仍受支持，以免写入已知无效配置。
28. 作为使用显式模型的用户，我希望每次分析前重新验证服务端能力，以免使用已经下线的模型。
29. 作为配置失效的用户，我希望分析失败并指引我修改设置，而不是静默改用默认模型。
30. 作为正在运行分析的用户，我希望修改偏好不取消当前任务，而只影响之后的新分析。
31. 作为阅读解释的用户，我希望不同模型和思考强度不复用彼此的缓存，以免来源混淆。
32. 作为小窗口用户，我希望设置二级导航和内容仍然可用，以便无需扩大窗口完成配置。

## Implementation Decisions

- Application Settings 是应用壳中的独立目的地。侧栏底部由 GitHub Identity 和圆形设置按钮组成；设置页记住进入前目的地并支持返回。
- 设置页从第一版开始采用左侧二级导航，包含外观、GitHub、宝宝模式，并允许以后增加分区。
- 建立一个设备级 Application Settings 应用边界，负责读取、校验、原子写入和版本迁移。配置文件位于应用数据目录，名称为 `settings.json`。
- 统一配置保存主题偏好、Codex Executable Preference 与 Babysit Model Preference。动态模型目录不保存。GitHub 密钥、token 和公开身份缓存不进入该文件。
- 迁移现有宝宝模式设置文件中的 Codex 可执行文件；迁移成功后以统一配置为权威来源，并保证重复启动迁移幂等。
- 主题类型为 system、light、dark，默认 system。有效主题同时驱动根节点主题类、Monaco 主题和品牌图像，不再由独立 media query 各自决定。
- 主题选择立即保存；写入失败恢复旧主题并返回可见错误。
- GitHub 设置复用现有 GitHub Identity lifecycle 和退出应用契约，不创建第二套退出状态机。设置页退出前确认，侧栏快捷退出保持无确认。
- 确认文案说明退出会删除本机 GitHub 授权凭据和身份/头像缓存，但不会删除 Repository、Review 或本地代码。
- Codex 可执行文件只能通过原生文件选择器配置。后端要求绝对路径、规范化路径并执行 `codex --version`；只有验证成功才原子替换旧值。
- 显式 Codex Executable Preference 拥有绝对优先级。它失效时返回可操作错误且不自动发现其他安装。清空后才按环境 PATH、平台安装位置自动发现。
- Review Reader 中现有的“选择 Codex 可执行文件”失败恢复操作被移除；错误指向 Application Settings。
- Babysit Model Preference 支持 automatic 或 explicit。automatic 每次分析使用 App Server 标记的默认模型及其默认思考强度。
- explicit 保存模型标识，以及“模型默认”或一个具体思考强度。选择另一模型时，思考强度重置为该模型默认选项。
- 设置页平时不启动 App Server。用户点击修改后才连接并分页调用 `model/list`，过滤隐藏模型，显示服务端名称、默认项、`supportedReasoningEfforts` 与 `defaultReasoningEffort`。
- 模型编辑采用显式保存和取消。保存只接受本次模型目录支持的组合；失败不覆盖旧值。
- 启动每次新分析前调用 `model/list` 验证 explicit 配置。模型不存在或思考强度不支持时分析失败、保留配置并引导进入设置；不得静默回退或写回其他值。
- 模型目录获取失败时保留当前保存值并显示重试，不提供内置列表或自由文本输入。
- 设置变化不取消正在运行的分析，只影响之后启动的分析。provider 身份、模型标识、解析后的思考强度和分析契约版本共同参与缓存身份。
- Tauri transport 继续使用统一 typed command contract；Application Settings、GitHub OAuth 与 Babysit 模块通过各自窄应用接口组合，不把设置页变成跨领域状态机。
- 新增或修改的方法、结构体和类提供易读中文注释；Rust 私有模块测试放入对应 `tests/` 目录的独立文件。

## Testing Decisions

- 最高测试缝是 Application Settings 应用契约：通过公开行为验证默认值、原子更新、错误回滚、旧宝宝模式配置迁移和幂等启动，不直接测试私有 JSON helper。
- Babysit provider 契约测试覆盖显式路径绝对优先、失效不回退、清空恢复发现、分页 `model/list`、自动默认、显式能力验证和稳定错误分类。
- 应用壳组件测试覆盖设置目的地、进入前页面恢复、二级导航、即时主题、GitHub 确认退出、模型编辑保存/取消和错误状态。
- GitHub 退出继续复用现有 identity entry、identity lifecycle 和 Tauri adapter 的先例测试。
- 模型协议 fixture 断言服务端模型、默认模型、支持思考强度与分页，不把当前本机模型名称硬编码为产品契约。
- 缓存行为测试证明模型或思考强度改变不会命中旧解释，同时不会取消已经运行的分析。
- 前端自动化之外，使用通用 Vite Preview Harness 和应用内浏览器检查宽窗口、窄窗口、三个设置分区、长路径、加载/空/错误状态、GitHub 确认弹层以及主题切换后的实际尺寸与视觉效果。
- 交付门禁包含 TypeScript 测试、类型检查、Lint、生产构建、`git diff --check`，以及通过 `scripts/cargo-agent.sh` 执行的聚焦 Rust 验证和最终一次 Rust 全量测试。

## Out of Scope

- GitHub Identity 登录或 OAuth 流程重设计。
- 同步设置到 GitHub、云端或其他设备。
- Repository 或 Review 级设置覆盖。
- 自由输入 Codex 路径、模型 ID 或思考强度。
- 内置模型目录、模型目录持久化缓存或离线编辑目录。
- 修改 Codex 身份验证、Codex home、权限、网络或审批策略。
- 修改正在运行的宝宝模式分析或自动重启分析。
- 手动语言选择器以及外观、GitHub、宝宝模式之外的新设置分区。

## Further Notes

- [ADR-0022](../adr/0022-reuse-the-reviewers-codex-environment.md) 继续约束复用用户 Codex 环境。
- [ADR-0023](../adr/0023-prefer-an-explicit-codex-executable.md) 取代其“手选路径仅为 fallback”的优先级决定。
- GitHub Identity 的既有行为由 [Global GitHub Identity Entry](./0005-global-github-identity-entry.md) 约束。
- Babysit provider 的既有行为由 [Babysit Mode](./0012-babysit-mode.md) 约束；本规格明确替换其中固定模型和失败现场选择路径的部分。

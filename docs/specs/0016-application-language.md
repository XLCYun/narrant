# Application Language

## Problem Statement

Narrant 已经离线打包简体中文和英文资源，但语言选择仍来自浏览器运行时与
`localStorage`，没有纳入设备级 **Application Settings**。因此用户无法在设置中明确
选择语言，前后端也没有同一个权威来源来决定界面和未来 **Line Explanations** 的语言。

## Solution

在 Application Settings 的外观分区增加简体中文与 English 两项明确选择，并把
**Application Language** 作为设备级持久化偏好。应用启动和切换时让现有 i18n 语言变量
跟随该偏好；Babysit 在每次生成开始时从后端 Application Settings 冻结语言，不再接收
前端传入的 locale。移除语言的 `localStorage` 读写，不迁移旧值。

## User Stories

1. 作为首次启动 Narrant 的用户，我希望应用按操作系统语言自动选择简体中文或英文，以便首次界面符合我的环境。
2. 作为使用中文的用户，我希望在外观设置中选择简体中文，以便已接入 i18n 的界面立即使用中文。
3. 作为使用英文的用户，我希望在外观设置中选择 English，以便已接入 i18n 的界面立即使用英文。
4. 作为已经明确选择语言的用户，我希望选择被设备级持久化，以便重启后继续使用该语言且不再随系统变化。
5. 作为从旧版本升级的用户，我希望缺少语言字段的设置仍可加载，以免现有主题与 Babysit 配置失效。
6. 作为过去使用浏览器语言偏好的用户，我希望旧 `localStorage` 值不再影响应用，以便 Application Settings 成为唯一权威来源。
7. 作为切换语言的用户，我希望保存成功后现有 i18n 变量和文档语言标记立即更新，以便无需重启即可看到变化。
8. 作为遇到保存失败的用户，我希望界面恢复原语言并显示错误，以免显示状态与持久化设置不一致。
9. 作为使用 Babysit 的用户，我希望新生成的 Line Explanations 使用后端当前保存的 Application Language，以免前后端语言漂移。
10. 作为在生成过程中切换语言的用户，我希望在途生成继续使用启动时冻结的语言完成，以免设置变化中断工作。
11. 作为已经看到 Line Explanations 的用户，我希望切换语言时旧结果继续保留，以免无意丢失已有内容。
12. 作为希望更新解释语言的用户，我希望只有在我主动重新生成时才使用新的 Application Language，以免切换设置自动产生模型调用。
13. 作为窄窗口用户，我希望语言设置项仍可完整操作，以便窗口宽度不会阻碍偏好修改。

## Implementation Decisions

- **Application Language** 是 Application Settings 中的 typed preference，仅允许简体中文与英文；设置 UI 只显示这两项，不提供“跟随系统”选项。
- 没有持久化语言时，后端系统语言解析器将中文系统映射为简体中文，将其他系统语言映射为英文。用户首次明确选择后保存具体值，不再跟随系统变化。
- 现有设置版本中缺失语言字段属于兼容输入；加载时补出系统解析后的语言，同时保留原有主题和 Babysit 配置。未知语言值仍视为无效设置。
- Application Settings 是语言的唯一权威来源。前端从设置快照驱动现有 i18n 实例和根文档 `lang`，不再从 `localStorage` 或浏览器直接选择运行语言。
- 删除 `narrant.locale` 的读取与写入；不迁移旧值。升级后缺少 Application Language 的用户重新按系统语言解析。
- 语言选择立即原子保存。保存成功后应用新语言；失败时恢复旧语言并显示可操作错误。
- Babysit 前端请求与 Tauri transport 不再携带 locale。后端在一次生成开始时，从同一 Application Settings 快照取得语言并写入 provider request 与解释缓存 identity。
- 已开始的 Babysit 生成冻结启动时语言；之后的设置变化不取消、不重启该运行。
- 语言切换不清除或重新生成已经显示的 Line Explanations。只有用户现有的明确重新生成操作才以当前设置语言建立新运行。
- 本功能只接通设置持久化、现有 i18n 语言变量与 Babysit 的权威语言来源；不补充翻译资源，也不改造尚未接入 i18n 的硬编码文案。
- 新增或修改的方法、结构体、类和组件提供易读中文注释；Rust 私有测试继续放在模块 `tests/` 目录。

## Testing Decisions

- 测试外部行为，不直接测试模块私有对象；优先复用 Application Settings、应用壳与 Tauri 命令的既有公开 seam。
- Application Settings 契约测试覆盖缺失语言字段的系统默认、中文与英文原子持久化、非法值，以及保存失败时原快照不变。
- 应用壳 UI 测试通过公开 Application Settings port 覆盖立即切换 i18n、更新根文档 `lang`、持久化失败回滚，以及不再读写语言 `localStorage`。
- Babysit Tauri 命令测试覆盖 transport 不接受 locale、后端冻结设置语言、在途运行不受切换影响，以及主动重新生成使用新语言。
- 继续保留 provider-independent 的 Line Explanation 契约；无需为每个 provider 重复验证同一设置读取行为。
- 使用通用 Vite Preview Harness 和应用内浏览器真实检查宽、窄窗口下的中文、英文、切换交互与保存失败状态，同时检查可访问 DOM 和关键元素尺寸。
- 完成前运行受影响前端测试、类型检查、Lint、生产构建，以及按仓库 Cargo 节奏执行 Rust 聚焦测试和一次全量测试；不手动触发 quality-gate。

## Out of Scope

- 新增简体中文和英文之外的语言。
- 提供“跟随系统”作为可选的持久化偏好。
- 补齐或重写现有硬编码文案的翻译。
- 迁移旧 `narrant.locale` 值。
- 切换语言时自动取消、清除或重新生成 Line Explanations。
- 改变 provider 的生成内容契约、支持的代码语言或模型配置。

## Further Notes

- 现有 i18n 资源和 Babysit locale-aware prompt 是本功能的基础；本规格只改变它们的权威输入来源。
- **Application Language** 已加入根领域词汇表，避免将其误称为独立的 Babysit Language 或 Comment Language。

# Issue #154: Application Language Settings

## Problem Statement

Application Settings 尚未保存 **Application Language**，现有 i18n 初始化仍读取浏览器与
`localStorage`。用户无法从外观设置明确选择简体中文或英文，运行时语言也可能与设备级设置
快照不一致。

## Solution

扩展 Application Settings 公共契约，加入仅含简体中文与英文的 typed language；兼容旧
settings v1，并在缺少字段时通过可测试的系统语言解析边界补出默认值。外观设置提供两种语言
选择，成功保存后立即驱动现有 i18n 和根文档语言，失败则回滚。完全删除语言 localStorage
读写，不迁移旧值。

## User Stories

1. 作为首次启动用户，我希望中文系统默认显示简体中文，其他系统默认显示英文。
2. 作为升级用户，我希望旧设置缺少语言字段时仍保留已有主题和 Babysit 配置。
3. 作为中文用户，我希望在外观设置选择简体中文并立即应用。
4. 作为英文用户，我希望在外观设置选择 English 并立即应用。
5. 作为已选择语言的用户，我希望重启后继续使用保存的明确语言。
6. 作为保存失败的用户，我希望恢复旧语言并看到错误。
7. 作为旧版本用户，我希望废弃的 localStorage 值不再覆盖 Application Settings。
8. 作为辅助技术用户，我希望根文档的语言标记与当前界面语言一致。
9. 作为窄窗口用户，我希望语言选择仍完整可用。

## Implementation Decisions

- Application Language 只允许简体中文和英文，序列化值保持稳定且与现有 i18n 白名单可无损映射。
- settings v1 继续兼容：缺失 language 时由注入式系统 locale resolver 解析，不把动态系统判断藏在静态类型默认值里；未知显式值仍拒绝加载。
- 中文 locale family 映射为简体中文；所有其他系统 locale 映射为英文。
- 设置公共快照、更新命令、前端 port 和 Tauri adapter 同步增加语言字段与原子更新操作。
- 应用启动只加载一个一致的设置快照来驱动主题和语言，避免独立 hook 重复加载产生竞态。
- 成功更新后调用现有 i18n 实例切换语言并同步根文档 `lang`；失败保留旧快照、旧语言和旧文档标记。
- 外观分区沿用现有 shadcn 组件体系增加简体中文与 English 选择，不引入新基础组件。
- 删除 localePreferenceKey、localStorage 初始化分支和测试辅助中的持久化行为；不读取、不迁移旧值。
- 不新增翻译资源，不处理硬编码中文文案。
- 新增或修改的方法、结构体、类和组件提供易读中文注释；Rust 私有测试放在模块 tests 目录。

## Testing Decisions

- Rust Application Settings 公开契约测试覆盖缺字段的中文/非中文系统解析、显式值 round trip、非法值和写入失败原快照不变。
- 前端应用壳 seam 使用公开 Application Settings port 覆盖启动同步、选择两种语言、`html.lang`、保存失败回滚和 localStorage 不参与。
- 设置页组件测试覆盖可访问名称、选中状态和错误状态，不直接断言私有 hook 内部实现。
- Preview Harness 提供中文、英文与保存失败场景；应用内浏览器检查宽窄 viewport、可访问 DOM、控件实际尺寸与交互。
- 运行受影响测试、类型检查、Lint、生产构建；Rust 命令统一通过仓库 Cargo wrapper。

## Out of Scope

- Babysit transport 与后端生成语言读取；由 Issue #155 交付。
- 第三种语言或“跟随系统”可选项。
- 旧 localStorage 偏好迁移。
- 现有硬编码文案国际化。

## Further Notes

- 本 Issue 冻结 #155 将消费的 Application Language 设置契约。


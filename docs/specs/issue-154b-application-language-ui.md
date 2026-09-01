# Issue #154B: Application Language UI

## Goal

在现有 Application Settings 外观分区提供简体中文与 English 选择，使应用壳现有 i18n
实例和根文档 `lang` 以 Application Settings 快照为唯一权威来源。

## Scope

- 扩展 TypeScript `ApplicationSettings`、公开 application port、Tauri command contract 与 adapter，
  消费 Rust 分片冻结的 `language` / `update_application_language` 契约。
- 应用启动用同一 settings 快照同步主题与语言，成功切换后立即更新 i18n 与 `<html lang>`。
- 保存失败保留旧快照、旧 i18n 语言和旧文档语言，并显示可操作错误。
- 外观页沿用既有 shadcn 组件增加简体中文与 English 两项，不新增基础组件。
- 删除 `narrant.locale`、`localePreferenceKey` 及全部 locale localStorage 读写；不迁移旧值。
- 扩展通用 Preview Harness，提供中文、英文和保存失败代表性场景。

## Confirmed Test Seam

只通过公开 Application Settings port 和应用壳 UI seam 测试。覆盖启动同步、两种语言选择、
`html.lang`、失败回滚、可访问名称/选中状态，以及 localStorage 不参与；不测试私有 hook。

## Constraints

- 使用 worker-mode 与 TDD red-green 垂直切片，只运行受影响的前端聚焦测试。
- 新增或修改的方法、结构体、类和组件有易读中文注释。
- 不补充翻译资源、不国际化硬编码文案、不实现 Babysit transport。
- 不执行最终全量测试，不触发 quality-gate；真实浏览器验收由雄蜂在集成后完成。

## Done

聚焦前端测试通过，提交干净 commit，并向雄蜂报告命令与测试数量、Preview 场景入口。

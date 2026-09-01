# Issue #154A: Application Language Contract

## Goal

在 Rust Application Settings 公共契约中加入仅支持简体中文与英文的
**Application Language**，兼容缺少该字段的 settings v1，并暴露 typed Tauri 更新命令。

## Scope

- `ApplicationSettings` 快照新增稳定序列化的语言字段。
- 语言值只允许 `zh-CN` 与 `en-US`；未知显式值拒绝加载。
- 缺少语言字段时通过可注入、可测试的系统 locale resolver：中文 locale family 解析为
  `zh-CN`，其他 locale 解析为 `en-US`，同时保留旧设置的其他字段。
- 新增原子 `update_language` 应用操作和对应 Tauri command；写入失败时旧快照仍为权威。
- 保持 settings schema version 为 1，供 Issue #155 后续消费同一 typed 字段。

## Confirmed Test Seam

只通过公开 `ApplicationSettingsApplication` 和 typed Tauri command 契约测试，不直接测试
JSON、路径或 resolver 私有 helper。覆盖缺字段的中文/非中文默认、显式值 round trip、非法值、
原子写入失败回滚和命令映射。

## Constraints

- 使用 worker-mode 与 TDD red-green 垂直切片。
- 所有 Rust 命令通过 `scripts/cargo-agent.sh`；只运行聚焦测试，不运行全量测试。
- Rust 私有测试位于模块 `tests/` 目录；新增或修改的方法、结构体有易读中文注释。
- 不修改前端，不实现 Babysit 语言读取，不触发 quality-gate。

## Done

聚焦 Rust 测试通过，提交干净 commit，并向雄蜂报告命令与测试数量。

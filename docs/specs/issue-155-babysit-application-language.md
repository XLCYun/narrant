# Issue #155: Babysit Application Language

## Problem Statement

Babysit 目前由前端把 i18n locale 直接传入生成命令。即使 Application Settings 已保存
**Application Language**，后端仍可能接受与设备设置不一致的语言，无法成为权威生成来源。

## Solution

从前端请求和 Tauri transport 删除 locale。每次 Babysit 生成开始时，后端从同一
Application Settings 快照取得并冻结 Application Language，再用于 provider request 和缓存
identity。设置切换保留旧结果且不触发模型调用，在途运行继续完成；用户主动重新生成时才使用
新语言。

## User Stories

1. 作为 Babysit 用户，我希望新生成的 Line Explanations 使用后端保存的 Application Language。
2. 作为切换语言的用户，我希望旧解释继续显示且不自动产生模型调用。
3. 作为在生成时切换语言的用户，我希望当前运行按启动时语言继续完成。
4. 作为需要新语言解释的用户，我希望主动重新生成后得到当前设置语言的结果。
5. 作为维护者，我希望前端无法通过 transport 覆盖权威语言。
6. 作为重复生成用户，我希望不同语言使用不同缓存 identity，避免返回错误语言结果。

## Implementation Decisions

- 前端 Babysit request identity、生命周期调用、Tauri command contract 和 adapter 全部删除 locale 参数。
- Tauri 命令在 provider 运行开始前读取一次 Application Settings 快照，从中取得语言；同一次运行不再重读。
- 后端把冻结语言映射为 provider request 的 locale，并继续纳入完成缓存 identity。
- Application Settings 读取失败作为生成启动失败返回，不回退到前端或隐式默认语言。
- 前端语言切换不再 invalidates 当前解释，不取消运行，也不自动调用 adapter。
- 现有明确“重新生成逐行解释”操作仍建立新请求；后端此时读取最新 Application Language。
- provider prompt、修复和输出验证契约保持不变；不为 provider 增加独立语言偏好。
- 新增或修改的方法、结构体和类提供易读中文注释；Rust 私有测试放在模块 tests 目录。

## Testing Decisions

- transport contract 测试证明 explain command 不再接受 locale，adapter 不再发送该字段。
- Tauri command/application seam 使用可控 Application Settings 验证中文与英文冻结、设置读取失败和缓存隔离。
- 前端 Babysit 生命周期测试证明 i18n 改变不清除已完成结果、不取消在途请求、不自动生成。
- 主动重新生成测试证明下一次后端调用读取最新语言；不直接测试 provider 私有实现。
- 运行受影响前端测试、Rust 聚焦测试、类型检查、Lint 和生产构建。

## Out of Scope

- Application Language 设置 UI 与持久化契约本身；由 Issue #154 交付。
- 自动重新生成或翻译已有 Line Explanations。
- 修改 provider-independent 输出 schema、代码语言 eligibility 或模型设置。
- 补充界面翻译资源。

## Further Notes

- Issue #155 blocked by #154，因为它消费后者冻结的 Application Language 设置契约。

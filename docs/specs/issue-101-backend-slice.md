## Problem Statement

Babysit Mode 仍可能忽略用户在 Application Settings 中明确选择的 Codex 可执行文件，且保存前缺少统一的规范化与版本验证边界。

## Solution

扩展 Application Settings 与 Babysit provider 公共契约：只接受绝对、canonical、可执行且能成功响应 `codex --version` 的文件；显式偏好存在时只使用该文件，失效时返回可操作错误；清空后才恢复自动发现。

## User Stories

1. 作为安装多个 Codex 的用户，我希望明确选择的可执行文件始终优先，以便运行行为与设置一致。
2. 作为选择错误文件的用户，我希望保存前得到验证错误，以便旧配置不被覆盖。
3. 作为显式配置失效的用户，我希望分析失败而不回退，以便不会静默使用另一安装。
4. 作为恢复默认行为的用户，我希望清空偏好后重新使用自动发现。

## Implementation Decisions

- Application Settings 应用契约负责绝对路径、canonical 文件与 `codex --version` 验证，并只在验证成功后原子更新快照。
- 验证必须限制运行时间和输出大小，错误不得泄露敏感路径或原始进程输出。
- Babysit provider 从统一 `settings.json` 读取偏好；显式偏好存在时不枚举 PATH 或平台位置。
- 清空偏好后沿用 PATH 与平台安装位置的现有自动发现。
- 新增或修改的 Rust 方法与结构体使用中文注释；私有模块测试位于模块 `tests/` 目录。

## Testing Decisions

- 在 Application Settings 公共契约测试保存成功、相对路径拒绝、版本探测失败不覆盖旧值、canonical 路径落盘与清空。
- 在 Babysit provider 公共契约测试显式路径绝对优先、显式路径失效不回退，以及清空后的自动发现。
- 严格按 red-green 垂直切片推进，不直接测试私有 helper。

## Out of Scope

- 原生文件选择器与设置页 UI。
- 模型与思考强度配置。
- Codex home、认证或网络配置。

## Further Notes

- 受 `docs/specs/0014-application-settings.md` 与 ADR-0023 约束。

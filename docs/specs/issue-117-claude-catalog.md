# Claude Code 离线模型目录分片

## Problem Statement

Claude Code 的 Babysit Agent Provider 仍使用固定模型与思考强度，Application Settings
无法通过 provider-neutral 模型目录安全地配置 Claude Code。

## Solution

提供不启动 CLI、不会发起付费请求的离线模型目录，解析并验证 Claude Code 的模型与
reasoning effort 偏好，同时保持自定义云 provider 模型标识。

## User Stories

1. 作为 Claude Code 用户，我希望选择官方滚动 alias，以便无需固定模型版本。
2. 作为追求最强能力的用户，我希望选择 Best（使用最强的可用模型）。
3. 作为云 provider 用户，我希望保存任意非空模型标识，以便使用部署名称或 ARN。
4. 作为 Haiku 用户，我希望界面只提供实际支持的默认 effort，以免保存无效组合。
5. 作为自定义模型用户，我希望选择完整 effort 列表，以便按目标部署调节能力。

## Implementation Decisions

- Claude Code 的稳定 provider ID 是 `claude-code`。
- 目录包含 Default、Best、Fable、Opus、Sonnet、Haiku、Custom。
- 排除 opusplan、扩展上下文 alias 与 ultracode。
- null 模型或 effort 表示不覆盖 CLI 默认值。
- Custom 只做 trim 后非空校验，原始非空标识按用户输入保存。
- Haiku 仅允许 Default；Custom 允许 Default、Low、Medium、High、XHigh、Max。
- 目录是随 Narrant 发布的离线能力，不调用 Platform Models API，不读取 OAuth 凭据，也不抓取 TUI。

## Testing Decisions

- 通过 provider-neutral 模型目录与 resolver 公开行为测试目录内容、稳定 ID 和组合校验。
- 测试只观察返回目录与解析结果，不直接测试私有表或 helper。
- 使用确定性单元测试，不依赖 Claude 安装、账户、网络或付费请求。

## Out of Scope

- 在线发现 Claude 账户实际可用模型。
- API key 或 OAuth credential 管理。
- 执行 Claude generation/repair 进程。

## Further Notes

受 `docs/specs/0012-babysit-mode.md`、`docs/specs/0014-application-settings.md` 和
ADR-0025 约束。

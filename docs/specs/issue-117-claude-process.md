# Claude Code 能力模块与配置执行分片

## Problem Statement

Claude Code adapter 将编排、命令构造、协议解析与进程生命周期集中在一个模块中，且
generation/repair 尚未可靠使用每次分析冻结的模型与 effort 设置。

## Solution

按 Claude Code 能力边界拆分 orchestration、discovery、runtime、model catalog、process
和 protocol，并让每个 fresh、non-persistent generation/repair 进程使用同一份 resolved
settings 快照。

## User Stories

1. 作为用户，我希望显式模型与 effort 精确传给 Claude Code，以便分析遵守设置。
2. 作为使用默认值的用户，我希望对应 CLI flag 被省略，以便沿用 Claude Code 默认行为。
3. 作为修复轮次用户，我希望 repair 与 generation 使用相同冻结配置，以免一次分析中途变更模型。
4. 作为隐私敏感用户，我希望每轮保持无状态且不写入 Claude session history。
5. 作为 Windows 开发者，我希望测试 fixture 不触发 `.sh` 文件关联窗口。

## Implementation Decisions

- `claude/mod` 只负责编排和 provider 契约；discovery、runtime、model catalog、process、protocol 各自拥有一个能力边界。
- generation 与所有 repair round 共用分析开始时冻结的 resolved model/effort。
- null 模型不传 `--model`；null effort 不传 `--effort`；显式值原样作为独立参数传递。
- 保持 `--no-session-persistence`、结构化输出与只读工具策略。
- 不引入付费请求、credential 提取、TUI 抓取或 Platform Models API。
- Windows 测试通过明确 shell executable 启动 fixture，不把 `.sh` 当作可执行程序交给系统关联。

## Testing Decisions

- 通过 `BabysitAgentProvider` generation/repair seam 与确定性 CLI 事件日志观察真实参数。
- 以纵向 TDD 切片覆盖默认 flag 省略、显式 flag 精确传递、repair 配置冻结和 Windows fixture 启动。
- Rust 私有模块测试镜像放在 `babysit/tests/claude/` 下，不与生产实现同文件。

## Out of Scope

- OS 级文件系统或网络 sandbox。
- Claude session 文件删除或非公开 raw streaming protocol。
- 设置页 UI 与 provider registry 装配。

## Further Notes

受 `docs/specs/0012-babysit-mode.md`、ADR-0024 与 ADR-0025 约束。

## Agent skills

### Backlog

Implementation tickets live in this repository's GitHub Issues. Product requirements and
specifications live in `docs/specs/`. See `docs/agents/backlog.md`.

### Triage labels

Uses the default five-label vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout. See `docs/agents/domain.md`.

## 前端 UI 组件
1. 如果 shadcn 提供的默认组件满足要求，使用 shadcn 的（如果未安装下载组件，则下载后使用）
2. 否则，如果能在 shadcn 组件基础上定制，则定制。
3. 否则，才新增组件

## 风格指南
1. 对方法、结构体、类等，都需要添加易读的中文注释。
2. 模块的私有对象的单元测试不允许直接与代码放在同一个文件中， 需要单独放在测试文件中。

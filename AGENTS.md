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

## 前端可视化验收
1. 涉及布局、响应式、弹层、截断或交互状态的前端改动，在自动化测试之外还需要使用应用内浏览器进行真实渲染检查。
2. 当完整应用依赖 Tauri 状态而无法直接在浏览器复现时，优先运行 `pnpm dev:preview`，在仓库的通用 Vite Preview Harness 中用代表性 props 和数据渲染目标组件；不得为了预览修改真实业务数据。
3. 预览至少覆盖与需求相关的宽窗口、窄窗口、长短内容、弹层展开和关键交互状态。优先同时检查截图、可访问 DOM 与关键元素的实际尺寸和位置，不能只凭静态代码判断视觉结果。
4. 响应式检查使用浏览器 viewport 能力设置明确尺寸；结束后恢复默认 viewport。
5. `preview.html`、`src/dev/preview.tsx` 和可复用的代表性场景属于通用开发基础设施，可以提交。若通用 Harness 暂时无法覆盖而创建一次性预览页，完成检查后必须删除，不得进入最终提交。
6. 浏览器验收不能替代单元测试、类型检查、Lint 和生产构建。

## 风格指南
1. 对方法、结构体、类等，都需要添加易读的中文注释。
2. 模块的私有对象的单元测试不允许直接与代码放在同一个文件中， 需要单独放在测试文件中。
3. Rust 模块的测试文件统一收纳在模块内的 `tests/` 目录中，并尽量保持与生产模块一致的目录层级；通过 `#[path = "..."]` 在对应生产模块中声明测试，参考 `src-tauri/src/persistence/tests/`。

## 阅读模式(read: {file})
根据给出的源代码文件 file，读取里面的内容，整理它的 mermaid 类图输出，并逐行添加注释后输出。直接输出 markdown 即可，不要写入到任何文件。

## Subagent Handoff 开发模式
使用 subagent + worktree + implement skill 的方式开发代码，需要给出详细的 spec 给 subagent。
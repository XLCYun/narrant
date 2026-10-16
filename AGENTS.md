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

## Swarm Coding 配置
1. Queen 使用 Sol Low
2. Drone 使用 Sol Low
3. Worker 使用 Luna Max

## Cargo 验证节奏
1. Agent、subagent 和 linked worktree 运行 Cargo 时，统一通过 `scripts/cargo-agent.sh` 获取仓库级全局锁并接入 sccache；不要并发启动多个冷 Rust 编译。完整说明见 `docs/agents/rust-build-cache.md`。
2. 实现期间优先使用 `scripts/cargo-agent.sh check --lib` 和受影响模块的聚焦测试。
3. 每个 Issue 收尾安排一次 Rust 全量测试，执行位置按下文验证节奏决定；审查小修只重跑受影响测试，除非修改了共享契约或测试暴露全局回归。
4. main 与仍在开发的 worktree 都保留 Cargo incremental；不要设置 `CARGO_INCREMENTAL=0`，除非是 CI 或有基准支持的一次性构建。
5. 不同分支保持各自的 `target`，不得让 divergent worktree 共享一个 `CARGO_TARGET_DIR`。
6. Issue 合并且 worktree 不再使用后，清理该 worktree 的 `src-tauri/target`，避免生成物长期占用磁盘。
7. 使用 `pnpm rust:cache:status` 查看 sccache，使用 `pnpm rust:cache:clean` 只清理 sccache；不要用清理缓存替代正常的增量构建。

## 本地验证与 CI 节奏
产品仅支持 Windows x64 和 macOS ARM64；通用检查与桌面测试共用现有自托管 runner 和当前已登录的桌面账户，不新增虚拟机、独立系统、账户、登录会话或 runner 注册。CI 继续使用独立工作目录与缓存；E2E 必须使用独立应用身份、测试数据和内存凭据。不得关闭、覆盖或清理日常 Narrant 的进程、安装、配置及数据。升级验收使用独立的 Narrant Upgrade Test 身份、内存凭据与进程级 TLS；正式包只做签名和完整性核验。不得将测试包结果描述为正式包本体升级成功。旧的系统 hosts、证书信任和正式安装测试入口继续阻止；规格与执行范围见 `docs/specs/0017-application-updates.md`。

1. 日常修改先在本地运行格式、类型检查、`check --lib` 和受影响测试；UI 改动仍按上文完成真实渲染检查。
2. 功能基本完成后，进行实际运行验证和代码审查；涉及 Windows 路径、进程、权限或打包时，提前运行对应 Windows 专项 CI，不把平台问题全部留到最后。
3. 准备合并时，对最终提交集中完成必要的 macOS、Windows CI。核对具体测试命令和覆盖范围；支持包构建或 runner 接入检查不能替代全仓或真实应用验收。
4. 同一提交的完整测试已由 CI 覆盖并通过时，本地无需重复执行；CI 未覆盖的检查仍在本地补齐。失败修复后重跑受影响范围，共享契约变化或全局回归需要扩大验证。
5. 自托管用于节省开支，保留测试和发布校验；工作流默认使用自托管 runner，离线时排队，不自动回退到付费托管机器。开发推送按阶段集中，必要时使用仓库支持的跳过 CI 机制，最终验收恢复正常执行。
6. 本机兼作 runner 时，开发和 CI 使用独立 checkout、target 与缓存，Cargo 仍通过 `scripts/cargo-agent.sh` 共用编译锁。调整调度、账户、桌面验收或清理时，先阅读 `docs/agents/macos-self-hosted-ci.md` 和 `docs/agents/windows-self-hosted-ci.md`。

## 不要手动触发 quality-gate
不要手动触发 quality-gate.

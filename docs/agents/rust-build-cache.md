# Rust worktree 构建与缓存

Narrant 的本地 Rust 构建同时使用三层机制：每个 worktree 独立的 Cargo `target`、
开发 profile 默认开启的 incremental，以及跨 worktree 共享的 sccache。Cargo Agent
命令继续通过 Git common dir 中的仓库级锁串行执行，避免多个冷构建同时争抢资源。
决策背景见 [ADR-0027](../adr/0027-use-isolated-incremental-targets-with-sccache.md)。

## 首次准备

macOS 安装：

```bash
brew install sccache
```

Windows 安装：

```powershell
winget install Mozilla.sccache
```

安装后检查：

```bash
pnpm rust:cache:status
```

如果没有安装 sccache，仓库脚本会明确警告并继续普通 Cargo 编译，不会阻断构建。
本仓库默认把本地缓存上限设为 2 GiB；如需临时覆盖，可在命令环境中设置
`SCCACHE_CACHE_SIZE`。

## Agent 与 worktree 规则

所有 Agent、subagent 和 linked worktree 都遵循以下规则：

1. Cargo 命令统一通过 `scripts/cargo-agent.sh`，例如：

   ```bash
   scripts/cargo-agent.sh check --manifest-path src-tauri/Cargo.toml --lib
   scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --lib module::tests
   ```

2. Tauri 命令通过 package script，日常运行使用 `pnpm td`，构建使用
   `pnpm tauri build ...`。不要绕过为 `pnpm exec tauri ...`，否则不会注入仓库的
   sccache 环境。
3. main 和仍在开发的 worktree 都保留 Cargo incremental；不要设置
   `CARGO_INCREMENTAL=0`。只有 CI 或确认只构建一次且有基准数据支持的环境可以关闭。
4. 每个 worktree 保持自己的 `src-tauri/target`。不要通过 `CARGO_TARGET_DIR` 指向
   main 或另一个分支的 target。
5. Issue 合并且 worktree 不再使用后，清理该 worktree 的 target；不要把
   `cargo clean` 当作普通故障恢复步骤。

## 查看与清理 sccache

查看缓存路径、2 GiB 容量限制和命中统计：

```bash
pnpm rust:cache:status
```

只有在缓存疑似损坏、需要回收空间或切换了不兼容工具链后才清理：

```bash
pnpm rust:cache:clean
```

该命令先停止 sccache 本地服务，再验证缓存路径并删除缓存内容。它不会删除任何
worktree 的 `target`，因此不会丢失 Cargo incremental 数据。清理后下一次构建会重新
填充 sccache，但同一 worktree 现有的 incremental 编译仍可继续使用。

不要手工递归删除从环境变量拼出的路径。若自定义 `SCCACHE_DIR`，路径名称必须明确
包含 `sccache`，否则仓库清理脚本会拒绝执行。

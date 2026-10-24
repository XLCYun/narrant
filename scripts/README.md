# 仓库脚本

脚本按维护用途分目录，命令入口和它依赖的模块放在同一组。新增脚本优先进入对应目录；
只有实际跨组共享、值得独立维护的逻辑才另设公共模块。

```text
scripts/
├── cargo-agent.sh   # 保留统一 Cargo 锁与缓存入口
├── dev/            # 本地启动、Rust 缓存、图标生成与人工 E2E
├── codegraph/      # 固定公开产物下载与 Narrant 消费验收
├── language-packs/ # 目标项目 SDK 夹具与共享进程回归
├── release/        # 应用版本、签名构建、Draft 与 Stable 发布
├── update-smoke/   # 正式包升级验收、平台适配、TLS 与证据
└── tests/          # Node 脚本及 workflow 回归测试
```

## 常用入口

从仓库根目录运行，已有 pnpm 命令名保持不变：

| 用途 | 入口 |
| --- | --- |
| 日常开发 / Tauri | `pnpm td` / `pnpm tauri ...` |
| Rust 检查 | `scripts/cargo-agent.sh check --manifest-path src-tauri/Cargo.toml --lib` |
| 缓存状态 / 清理缓存 | `pnpm rust:cache:status` / `pnpm rust:cache:clean` |
| 生成文件图标 | `pnpm generate:file-icons`（dev、preview、build 自动调用） |
| 下载固定 CodeGraph 七包 | `pnpm codegraph:download .tools/codegraph-packs` |
| 验收 CLI、diff、安装与 Review | `pnpm codegraph:verify .tools/codegraph-packs` |
| Windows 安装包 / 本地 macOS 签名包 | `pnpm build:windows` / `pnpm build:macos:signed` |
| 准备应用版本、提交和 tag | `pnpm release:prepare <不带 v 的版本>`（不会推送） |
| 脚本与 workflow 回归 | `pnpm test:scripts` |
| 人工真实模型 E2E | `pnpm test:e2e:custom-live`，见[使用条件](../docs/agents/babysit-smoke.md) |

发布和升级验收由现有 workflow 编排，具体参数、审批和恢复步骤见
[发布运维](../docs/application-update-operations.md)与[语言支持包说明](../docs/language-support-packs.md)。
Draft 和 promotion 脚本会写远端状态，不属于本地回归测试入口。
正式安装升级只在 GitHub-hosted 临时机器执行；日常 E2E 使用隔离身份和测试数据。

## 维护与验证

`tests/` 继续由 `node --test scripts/tests/*.test.mjs` 完整发现。语言包 workflow 还明确选跑部分文件；
增加或移动测试时同时核对默认命令和 CI 清单。发布准备与图标生成的真实 CLI 回归另外位于
`tests/release-prepare.test.ts`、`tests/file-type-icons-build.test.ts`。CodeGraph helper 源码及构建发布在独立仓库。

Cargo 保持经根目录的 `cargo-agent.sh` 进入，缓存实现位于 `dev/rust-cache.mjs`；
每个 worktree 继续使用独立 target。脚本的 cwd、参数、环境、输出与运行时产物名称属于调用契约，
修改目录时同时检查 import、动态 import、spawn、Swift/Node 子进程资源和仓库根定位。

完整清单、调用关系、迁移依据及后续重构候选见
[0026 — 脚本目录组织](../docs/specs/0026-scripts-organization.md)。

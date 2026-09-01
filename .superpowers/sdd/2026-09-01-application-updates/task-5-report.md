# Task 5 report — signed cross-repository Draft updater assets

## Outcome

Task 5 已实现并提交为 `9d9c765`（`feat(release): stage signed updater draft assets`）。实现只覆盖
私有源码仓库生成、签名、预验和协调公开 Draft 的能力，没有实现 Task 6 native smoke、TLS gate、
evidence、受保护 promotion 或运维 runbook，也没有创建、发布或修改任何真实 GitHub Release。

## Implementation

- `.github/workflows/release.yml`
  - macOS 与 Windows job 都在 Tauri build 前要求非空 updater 私钥、密码和公钥，并校验固定
    `NARRANT_UPDATER_ENDPOINT`；公钥同时进入 Rust 编译期 trust config 和 Draft 本地验签。
  - macOS 先生成 `.app`，递归 Developer-ID 签名，公证、staple 和验证最终 app；随后从最终字节
    重建并 updater-sign `.app.tar.gz`。DMG 从同一最终 app 单独创建、签名、公证和验证。
  - Windows 构建 current-user NSIS，分别收集公开 installer 名、updater `.exe` 和 `.sig`。
  - Draft job 先下载六个原生产物，在任何远端 mutation 之前生成 Release Notes、`latest.json`、
    `SHA256SUMS` 并完成全部本地预验；之后才通过 `CYUNLAB_RELEASE_TOKEN` 协调
    `cyunlab/narrant` Draft。
- `scripts/release-assets.mjs`
  - 提供稳定 tag/version、固定八资产名、原生 DMG/PE/tar.gz 容器、Tauri minisign packet 与
    Ed25519、精确资产集合和公开身份泄漏的统一 fail-closed 校验。
  - deterministic `latest.json` 只含 `darwin-aarch64`、`windows-x86_64`，使用 tag-specific HTTPS
    URL，保留 `.sig` 文件完整字面文本，复制公开 Release Notes，并省略 `pub_date`。
  - `SHA256SUMS` 在其余七文件齐备后生成，使用不依赖 locale 的公开名称排序，覆盖七文件且不
    覆盖自身。
- `scripts/reconcile-draft-release.mjs`
  - 把 GitHub CLI 作为可注入 adapter，只有明确 HTTP 404 才创建 Draft；其他 lookup 错误 fail
    closed。
  - 已有 Release 必须仍是 Draft 才会删除旧资产、更新公开 title/body 并完整上传八文件；
    published Release 在 mutation 前被拒绝。
  - 上传后通过 API 回读并证明 tag、Draft 状态和资产名称集合精确一致，支持同 tag 重试。
- `scripts/generate-public-release-notes.mjs` 与 `scripts/prepare-draft-assets.mjs`
  - 作为薄编排入口，把 Git/Actions 环境和文件系统输入交给上述深模块。
- `scripts/tests/`
  - 使用运行时生成的本地 Ed25519/minisign-compatible fixture，无生产密钥、网络或真实 GitHub
    mutation。
  - 覆盖稳定版本、两目标 manifest、literal 签名、有效/无效验签、缺少/意外/重复资产、
    URL/名称/版本/notes/签名错配、deterministic checksums、公开身份泄漏、Draft create/update/
    retry、非 404 lookup、published 拒绝、上传后 exact set，以及 workflow 静态契约。

## Verification

已执行：

- `node --test scripts/tests/release-assets.test.mjs`：7/7 通过。
- `node --test scripts/tests/reconcile-draft-release.test.mjs`：9/9 通过。
- `pnpm test:scripts`：25/25 通过（含现有 script tests 与新增 workflow 静态断言）。
- `pnpm lint`：通过。
- `node --check`：四个新增 production `.mjs` 入口/模块全部通过。
- Ruby Psych `YAML.parse_file(".github/workflows/release.yml")`：YAML 语法通过。
- `git diff --check` 与 `git diff --cached --check`：通过。

本机 Node 为 `v26.5.0`，因此 pnpm 对仓库声明的 `>=22.13 <26` 输出 engine warning；测试和 lint
均成功。工作流固定使用 Node 24，落在声明范围内。

按 Task 5 guardrail 未运行：

- 没有手动触发 `quality-gate`。
- 没有运行完整 Rust suite 或 Cargo 验证。
- 没有运行真实 macOS/Windows 签名、公证或 updater 构建。
- 没有访问或修改 secrets、variables、environment 或公开仓库。
- 没有创建或发布真实 Release。

## Residual risks

1. macOS Developer-ID/notary、Tauri signer 和 Windows NSIS `.exe.sig` 的真实 runner 路径只能在
   私有仓库 tag workflow 中用已配置密钥验证；本任务只完成了静态工作流断言和本地 fixture
   验签，未伪造远端成功证据。
2. GitHub Draft reconciliation 由多次 API/CLI mutation 组成，不具备跨调用事务或 CAS：管理员若
   在“确认 Draft”与后续删除/上传之间并发发布，GitHub 本身不能提供原子保护。本实现会在首次
   mutation 前拒绝已 published 状态，并在上传后再次要求 Draft/exact set；Task 6 的受保护
   promotion 仍必须二次回读身份、bytes/digests 和 evidence，作为最终发布门禁。
3. Task 5 的上传后证明按 scope 只回读精确资产名称集合；asset bytes、digest 和 native lifecycle
   的远端二次验证属于 Task 6，未在此提前实现。

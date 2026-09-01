# Task 6 fix round 1 report

## Outcome

Task 6 review 中的 1 个 Critical、5 个 Important 和 1 个 Minor 已修复。实现提交为
`791de884a8aeb8cbd1cd297d96193dbccf0a1f3a`（`fix(release): harden native update promotion`）。
本轮没有连接 GitHub、读取或修改真实 secret/variable/Environment/Release，没有发布制品，也没有手动
触发 quality-gate。

## Fixes

- direct `reqwest 0.12` 改为 `rustls-tls-native-roots`；仍关闭 default features，并保留 metadata/package
  两处 hostname/redirect allowlist。这样临时 CA 写入 hosted runner 系统 trust store 后，生产 HTTP
  client 才会在 Rustls 下读取同一 native root 集合。
- promotion 的 `gh attestation verify --signer-workflow` 改为 GitHub CLI 要求的无 scheme identity：
  `github.com/$GITHUB_REPOSITORY/.github/workflows/update-smoke.yml`。
- TLS gate 在任何 hosts mutation 前先生成原字节 backup 和 replacement，并立即 arm restore；即使实际写入
  后抛错也会恢复。只有 byte-for-byte 回读确认成功才删除 backup；恢复失败时 backup 保留供人工重试。
- bootstrap promotion 不再只看当前 Latest，而是以 token/header 有界分页扫描固定
  `cyunlab/narrant` 的 Release 历史（100 条/页，最多 20 页）。任一历史 `draft=false` Release 含
  `latest.json` 都永久关闭 bootstrap；后续页错误、重复/漂移 identity 和超过上限全部 fail closed。
- bootstrap recovery fixture 使用严格高于当前应用的 patch 版本，metadata 的 schema/target/installKind
  均有效，但包和签名必然无效。native driver 等待 production 清除 pointer 与 generation，并观察稳定的
  `transition-warning`/`verification` 诊断后才完成 clean close/relaunch 证明。
- 实现可执行的双公钥 old-key bridge：
  - `NARRANT_RELEASE_SIGNING_PUBLIC_KEY` 验证本版 updater package；
    `NARRANT_UPDATER_PUBLIC_KEY` 是候选 binary 内嵌的下一 trust root。
  - 普通发布要求两者相等；只有 `mode=normal` 且 `NARRANT_UPDATER_BRIDGE_TAG` 与 exact candidate tag
    相等时允许不同，bootstrap 永远拒绝 bridge。
  - `release.yml` 继续自动创建 Draft，不进入 protected Environment。repository variables 供 build/Draft/
    smoke 使用，只把两个 key 的 SHA-256 identity 和 `bridgeAuthorized` 写入 receipt/evidence，不写公钥正文。
  - native bridge 证明 baseline package/config 和 candidate package signature 使用 old release-signing key，
    更新后的 candidate 配置内嵌 successor key。Draft receipt、两个 target evidence、aggregate 和 promotion
    都绑定两摘要。
  - 只有最终 `stable-promotion` job 读取同名 Environment variables；它重新下载/验签 Draft、重算两摘要，
    并通过 receipt/evidence exact comparison 拒绝 stale、未授权 mismatch 或 bootstrap bridge，之后才允许
    fixed-ID PATCH。
  - runbook 给出 old-key bridge、受保护核对和下一 normal release 将 private/public keys 全部切到 successor
    并清空 bridge tag 的逐步流程。
- 两个 production `Transform.transform` 方法均补充独立中文注释。

## Verification

最终 implementation tree 的检查结果：

- `pnpm test:scripts`：`121 passed; 0 failed`。
- `pnpm lint`：通过。
- `scripts/cargo-agent.sh check --lib --manifest-path src-tauri/Cargo.toml`：通过，`17.78s`。
- `scripts/cargo-agent.sh test --lib application_update::metadata::tests:: --manifest-path
  src-tauri/Cargo.toml`：`4 passed; 0 failed`。
- `scripts/cargo-agent.sh test --lib application_update::runtime::tests:: --manifest-path
  src-tauri/Cargo.toml`：`10 passed; 0 failed`。
- `node --check`：13 个相关 production `.mjs` 全部通过。
- Ruby Psych `YAML.safe_load`：`release.yml`、`update-smoke.yml`、`promote-stable.yml` 全部通过。
- repository-owned workflow/static tests：9/9 通过，覆盖 exact two targets、cross-run permissions、
  attestation permissions/identity grammar、protected Environment、bootstrap fail-closed、双公钥变量和 token
  不进入 argv/URL。
- `git diff --check`、`git diff --cached --check`：通过。

按 fix package 要求，本轮只运行受影响 Rust 聚焦测试，没有运行 Task 7 保留的最终 full Rust suite。本机
Node `v26.5.0` 高于仓库声明的 `<26`，pnpm 输出 engine warning，但所有 Node tests/lint 均通过；hosted
workflows 固定使用受支持的 Node 24。Ruby 仍报告 `/opt/homebrew/bin` world-writable PATH warning，但 YAML
解析成功。本机没有安装 `actionlint`，因此没有联网安装，YAML parser 与 repository static tests 作为本轮
workflow 验证。

## Hosted/native residuals

1. 本地没有执行 macOS 15 arm64 / Windows 2025 x64 hosted runner 上的真实系统 trust-store、hosts/DNS、
   port 443 helper、signed previous Stable 安装、正常退出安装、无自动 relaunch 与手工 relaunch。首次受控
   native smoke 仍需证明这些 OS 行为和 runner 权限。
2. 本地没有实际调用 GitHub Draft asset API、跨 run artifact 下载、attestation verify、protected
   Environment reviewer、Release 历史分页或 fixed-ID PATCH/Latest readback；这些边界由 injectable fixture
   与静态 workflow contract 覆盖，最终 hosted run 才能提供平台证据。
3. 没有使用真实 old/successor keypair 构建 bridge binary。fixture 已证明 old-key package 验签、successor
   identity、未授权 mismatch/bootstrap 拒绝和 promotion exact comparison；首次计划轮换仍必须按 runbook
   在隔离环境先演练并核对两摘要。
4. 项目明确不启用 immutable releases。fixed-ID byte re-fetch、mutation 前 guard、Environment 审批与
   mutation 后 Latest readback 缩小并检测风险，但 GitHub mutable Release 仍没有 guard→PATCH 的原子 CAS；
   管理员必须在 smoke/promotion 窗口避免并发修改。

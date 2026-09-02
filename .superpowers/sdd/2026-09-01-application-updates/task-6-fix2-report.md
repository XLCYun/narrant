# Task 6 fix round 2 report

## Outcome

Task 6 round 1 复审剩余的 1 个 Important 和 1 个 Minor 已修复。实现提交为
`07b8739998880c5c48cdf40d2c7afe18f8e9a0b1`（`fix(release): bind previous archive signing key`）。
本轮没有连接 GitHub、读取或修改真实 secret/variable/Environment/Release，没有发布制品，也没有手动
触发 quality-gate。

## Fixes

- 新增 `NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY`，专门验证 previous Stable archive；它与
  `NARRANT_RELEASE_SIGNING_PUBLIC_KEY`（已安装版本信任的 candidate package key）以及
  `NARRANT_UPDATER_PUBLIC_KEY`（candidate binary 内嵌的下一 trust root）职责分离。
- previous key 只从环境进入内存。prepare/native 流程用它验证 previous Stable archive，`previous.json`、
  单 target evidence 和 aggregate evidence 只保存 `previousReleaseSigningKeySha256`，不落盘公钥正文。
- normal smoke 必须提供 previous key；bootstrap 必须省略并拒绝 previous key/previous Stable。workflow 在
  bootstrap 模式显式传空值，避免 repository variable 意外进入 bootstrap 证据。
- 两个 target 必须声明同一个 previous key SHA-256；aggregate verifier 对受保护期望做 exact comparison，
  拒绝缺失、漂移或 target 间不一致。
- 最终 `stable-promotion` Environment 在任何 Draft 下载或发布 mutation 前，从受保护
  `NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY` 重新计算摘要并核对 evidence；bootstrap 若 Environment
  配置了该 key 会 fail closed。
- 增加真实 Ed25519/minisign fixtures，完整证明 A `old/old` → B `old/successor` → C
  `successor/successor`：B 的 archive 只能由保留的 old previous key 验证，C admission 后才允许把
  previous key 切为 successor；使用 successor 提前验证 B 的负例因 key ID 不同而失败。
- runbook 明确 B/C/D 的三把 key 切换时序：C admission 期间 previous key 仍保留 old，C 成为 Stable 后才为
  D 切换到 successor；bootstrap 条件改为有界扫描全部历史 published Release，任一 `latest.json` 永久关闭
  bootstrap，不再使用“当前 Latest”措辞。

## Verification

最终 implementation tree 的检查结果：

- `pnpm test:scripts`：`128 passed; 0 failed`。
- `pnpm lint`：通过。
- `node --check`：13 个相关 production `.mjs` 全部通过。
- Ruby Psych `YAML.safe_load`：`release.yml`、`update-smoke.yml`、`promote-stable.yml` 全部通过。
- repository-owned tests 覆盖真实 A→B→C minisign 链、错误 successor 验 B 的负例、prepare 前置拒绝、
  bootstrap 拒绝、previous raw key 不落盘、双 target 摘要一致性、promotion protected recompute 和 workflow
  变量作用域。
- `git diff --check`、`git diff --cached --check`、implementation commit `git show --check`：通过。

本轮没有修改 Rust 或 Cargo 配置，按 fix package 要求没有重复运行 Cargo，也没有运行 Task 7 保留的最终
full Rust suite。本机 Node `v26.5.0` 高于仓库声明的 `<26`，pnpm 输出 engine warning，但所有 Node
tests/lint 均通过；hosted workflows 固定使用受支持的 Node 24。Ruby 仍报告 `/opt/homebrew/bin`
world-writable PATH warning，但三个 workflow 均成功解析。本机没有安装 `actionlint`，因此没有联网安装；
YAML parser 与 repository static tests 作为本轮 workflow 验证。

## Hosted/native residuals

1. 本地没有在 macOS 15 arm64 / Windows 2025 x64 上用真实签名安装包执行完整 A→B→C 生命周期，也没有
   真实证明 normal exit、安装、禁止自动 relaunch 和手工 relaunch 后的版本/key 状态。
2. 本地没有实际调用 GitHub Draft asset API、跨 run artifact/attestation、protected Environment reviewer
   或 fixed-ID promotion；previous key 摘要的 Environment 重算与 exact comparison 由 injectable fixture
   和 workflow/static contract 覆盖，仍需首次 hosted promotion 提供平台证据。
3. 没有用生产 old/successor keypair 构建和安装 A/B/C。真实 minisign fixtures 已证明密码学链与错误 key
   的拒绝，但首次生产轮换仍须按 runbook 在隔离环境演练并人工核对三个 key role 的摘要。
4. 项目明确不启用 immutable releases。fixed-ID re-fetch、evidence guard、Environment 审批与发布后
   Latest readback 仍不能提供 GitHub mutable Release 的原子 CAS；promotion 窗口应避免管理员并发修改。

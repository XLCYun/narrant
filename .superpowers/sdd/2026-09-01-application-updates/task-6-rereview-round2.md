# Task 6 fix round 2 re-review

## Verdict

**APPROVE**

- Range: `18875f4..f453682`
- Standards findings: 0
- Spec findings: 0
- Critical / Important / Minor: 0 / 0 / 0

Round 1 剩余的 A -> B -> C key-rotation 断点和 bootstrap runbook 旧描述均已闭合。

## Standards

PASS。未发现 documented-standard 违规或 actionable Fowler smell。

- `scripts/release-assets.mjs:272-285` 的 previous-key identity/policy helpers 有清晰中文职责注释；
  两个 production `Transform.transform` 的中文注释仍保留。
- 新测试均位于 `scripts/tests/`；本范围没有 Rust 测试布局变化。
- 三个 key role 在 workflow、driver 和 evidence 之间共同出现，表面上是 possible Data Clump / Shotgun
  Surgery，但它们是必须独立验证的信任边界，且 canonical 解析和摘要规则已集中在
  `scripts/release-assets.mjs`，不构成 finding。
- 本轮没有 Rust/Cargo 变更，无需重复运行 Cargo；未运行 full Rust suite 或 quality-gate。

## Spec

PASS。未发现缺失、部分实现、实现错误或 scope creep。

### A -> B -> C trust chain

- A：previous archive、current release package 与 installed updater configuration 均为 old key。
- B bridge：`NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY=old` 验 A archive，current release key 仍为
  old，candidate binary 内嵌 successor；只有 exact normal bridge tag 允许后两者不同。
- C next normal：previous key 继续为 old 并验证 B archive；current release key 已切为 successor，
  `scripts/native-update-platform-adapter.mjs:313-318` 用 successor 检查 installed B configuration；C package
  与 C embedded key 也为 successor。
- C 成为 Stable 后，D 才将 previous key 切为 successor。runbook 已明确该 B/C/D 时序。

真实 Ed25519/prehashed-ED minisign fixtures 覆盖 old 验 B 成功、successor 提前验 B 的 key-ID mismatch、
C successor/successor 成功，以及 missing previous key、wrong key、bootstrap 携 previous key 的负例。

### Trust identity flow

- `.github/workflows/update-smoke.yml:124-173` 的 normal prepare/native driver scope 显式传递独立 previous
  key；bootstrap 传空并由 admission 再次拒绝。
- `scripts/prepare-update-smoke-assets.mjs:80-127` 在下载前校验 canonical public key，用它验证完整 previous
  package，只把 `previousReleaseSigningKeySha256` 写入 `previous.json`。
- `scripts/run-native-update-smoke.mjs:53-140` 重算摘要、与 prepare receipt exact comparison，并只把摘要
  放进 per-target evidence；native child 从 environment 接收 raw key，不从 argv 或文件读取。
- `scripts/native-update-smoke-driver.mjs:15-35,118-135` 要求 normal key、拒绝 bootstrap key；platform adapter
  对 actual baseline bytes 再次用 previous key 验签。
- `scripts/update-smoke-evidence.mjs:183-222,254-293,346-381` 要求两个 target 的 previous key SHA-256
  与当前期望完全相同，bootstrap 不接受该 identity。
- `.github/workflows/promote-stable.yml:131-165` 在 protected `stable-promotion` Environment 中提供同名值；
  `scripts/promote-stable-release.mjs:148-155` 在第一次 Draft 下载或 mutation 前解析并重算摘要，随后 exact
  验证 attested evidence。
- Raw previous key 不进入 `previous.json`、per-target/aggregate evidence、artifact 或成功日志；持久化内容
  只有 canonical SHA-256 identity。

### Historical regression matrix

| Historical finding / contract | Result |
| --- | --- |
| reqwest Rustls trusted only WebPKI while gate installed a system CA | Closed; Cargo files unchanged and existing native-roots regression test passes. |
| `gh attestation verify --signer-workflow` used URL grammar | Closed; workflow retains `[host/]owner/repo/path`. |
| hosts post-write failure could skip restore | Closed; restore remains armed before mutation and backup survives restore failure. |
| bootstrap inspected only current Latest | Closed; bounded full Release-history scan remains fail closed, and runbook now matches it. |
| bootstrap fixture did not prove verifier/generation cleanup | Closed; strictly newer tampered fixture still requires pointer, generation and verification-diagnostic proof. |
| old-key bridge / next-normal rotation | Closed through independent previous/current/candidate key roles and A -> B -> C tests. |
| production Transform methods lacked Chinese comments | Closed; comments remain present. |
| exact targets, freshness, attestation, protected approval and fixed Release identity | Closed; existing evidence/promotion tests pass. |
| Task 5 published-asset mutation and byte/identity boundaries | No regression; full script suite passes. |

The accepted mutable GitHub Release fixed-ID guard-to-PATCH no-CAS residual is unchanged and is not reopened.

## Verification

- `pnpm test:scripts`: 128/128 passed.
- `pnpm lint`: passed.
- `node --check`: all seven changed production `.mjs` files passed.
- Ruby Psych: both changed workflow YAML files parsed.
- `git diff --check 18875f4..f453682`: passed.
- Standards independent axis: PASS, 0 findings.
- Spec independent axis: APPROVE, 0 findings; its focused review ran 45 relevant Node tests.

No network, real GitHub API/Release/secret/Environment, system trust-store/hosts mutation, native installer,
Cargo, full Rust suite, or quality-gate operation was used. Hosted native/GitHub proof remains an execution
residual and is not claimed by this local review.

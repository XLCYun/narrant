# Task 6 fix round 1 re-review

## Verdict

**NEEDS FIXES**

- Range: `9af8986..f883592`
- Critical: 0
- Important: 1
- Minor: 1

前一轮的 TLS trust source、attestation signer grammar、hosts 部分写入恢复、bootstrap 历史
关闭条件、bootstrap verification recovery 证明和中文注释问题均已闭合。双公钥实现可以完成
`A old/old -> B old/successor` bridge，但下一次 `C successor/successor` normal smoke 会把 C 的
successor key 错用于验证仍由 old key 签名的 B archive，因此计划轮换不能完整走通。

## Findings

### Important

1. **密钥轮换只完成 bridge 当轮，下一次 normal release 必然无法建立 previous Stable baseline**

   - `.github/workflows/update-smoke.yml:124-133` 的 previous Stable 步骤只注入当前候选 C 的
     `NARRANT_RELEASE_SIGNING_PUBLIC_KEY`。在 runbook 规定的下一 normal release 中，该值已经切换为
     successor。
   - `scripts/prepare-update-smoke-assets.mjs:83,113,135` 把这个当前 release signing key 直接用于
     previous B archive 的 minisign 验证；`scripts/native-update-platform-adapter.mjs:313-318` 安装前又
     使用同一个 key 重复验 B archive。
   - 完整序列的信任事实是：A package/embedded 都是 old；B package 仍由 old 签名，但 B binary 内嵌
     successor；C package/embedded 才都使用 successor。因此 C 必须用 old 验证 previous B archive，
     同时用 successor 检查 B 的 embedded updater configuration 及 C package。当前接口只有 C 的两个
     相同 successor inputs，没有 previous-release signing key。
   - 本地纯内存 Ed25519/minisign 反例分别建立 A、B、C 的 policy：old key 可验证 B archive；按当前 C
     路径改用 successor 后稳定失败为 `updater 签名 key id 不匹配`。现有
     `scripts/tests/release-assets.test.mjs:173-203` 只证明 bridge B 当轮，
     `scripts/tests/native-update-smoke-driver.test.mjs:48-68` 使用不做密码学验证的 fake adapter，均未覆盖
     A -> B -> C。
   - 影响：B 可以被 A 自动安装并成为 Stable，但随后的 C 两平台 smoke 在生成 evidence 前必失败，
     protected promotion 因而无法批准。运维文档要求的计划密钥轮换流程不可执行。
   - 最小修法：为 normal smoke 增加独立的 previous-release signing public key（与 current candidate
     signing key 分离），以受保护且按 previous tag 绑定的输入提供，或从每版持久、可信的公开记录恢复；
     把其 SHA-256 identity 加入 `previousStable` evidence 并在 protected promotion 中重算/核对。B archive
     用 previous key 验证，B installed configuration 继续用 successor 验证；artifact/evidence/log 只记录
     hash，不记录 raw key text。增加同时执行 A old/old、B old/successor、C successor/successor 的正例，
     以及 C 错用 successor 验 B 必须失败的反例。

### Minor

1. **bootstrap runbook 仍残留“只看当前 Latest”的旧描述**

   - `docs/application-update-operations.md:45-46` 已正确说明 promotion 会扫描任一历史 published
     updater Stable；但 `docs/application-update-operations.md:105-106` 仍称只要求“当前公开 Latest
     不得已有 latest.json”。
   - 影响：操作员可能误解永久关闭条件，与当前历史分页 fail-closed 实现不一致。
   - 最小修法：把该步骤改为“全部有界 Release 历史中不得存在含 `latest.json` 的 published Stable”。

## Prior finding disposition

| Prior finding | Result | Evidence |
| --- | --- | --- |
| Critical: direct reqwest used WebPKI roots while gate installed a system CA | Addressed | `src-tauri/Cargo.toml:31` uses `rustls-tls-native-roots`; wrapper `cargo tree -e features -i reqwest@0.12.28` resolves `rustls-tls-native-roots -> rustls_native_certs`; focused `check --lib` passes. |
| Important: `--signer-workflow` included an URL scheme | Addressed | `.github/workflows/promote-stable.yml:125-129` uses `github.com/owner/repo/path`; local `gh attestation verify --help` requires `[host/]owner/repo/path`. |
| Important: hosts write failure before `routed=true` skipped restore | Addressed | `scripts/update-smoke-tls-gate.mjs:86-99` arms restore before mutation; `scripts/update-smoke-tls-system-adapter.mjs:251-291` writes backup first, restores byte-for-byte, and retains backup after restore failure. Adversarial tests pass. |
| Important: bootstrap closure only inspected current Latest | Addressed in code | `scripts/promote-stable-release.mjs:99-136` scans 100 releases/page for at most 20 pages, rejects any historical published `latest.json`, and fails closed on malformed/duplicate/error/overflow pages. The Minor above is documentation only. |
| Important: bootstrap fixture only reached `NotNewer` and observed pointer deletion | Addressed | `scripts/native-update-platform-adapter.mjs:30-89` creates a strictly higher patch with invalid signature/package and requires pointer deletion, full generation deletion, and a `transition-warning`/`verification` diagnostic. |
| Important: one public key made old-key bridge impossible | Partially addressed; regression remains | Separate current package-signing and candidate-embedded keys correctly enable B, restrict inequality to an exact normal bridge tag, and bind only hashes. The Important above shows the required B -> C continuation is missing. |
| Minor: two production Transform methods lacked Chinese comments | Addressed | `scripts/prepare-update-smoke-assets.mjs:54-64` and `scripts/public-draft-assets.mjs:207-217` now document their chunk-limit/digest responsibilities. |

## Standards axis

No Standards findings. New production methods have readable Chinese responsibility comments, tests remain under
`scripts/tests/`, Cargo commands use `scripts/cargo-agent.sh`, and no full Rust suite or quality-gate was run.
The paired key fields are justified trust-boundary data rather than an actionable Fowler Data Clump.

## Spec axis

The release/Draft byte verification, fixed endpoints and targets, native-root TLS alignment, evidence freshness,
protected promotion hash recomputation, bootstrap permanent closure, and Task 5 contracts remain aligned with
Spec 0017 and ADR-0028. The Important finding violates Spec 0017's requirement that every release after bootstrap
prove a real previous-Stable upgrade and the fix-round package's explicit A -> B -> C proof. The Minor finding is a
runbook consistency defect.

The accepted fixed-ID mutable Release guard-to-PATCH no-CAS residual is unchanged and is not reopened.

## Verification

- `pnpm test:scripts`: 121/121 passed.
- `pnpm lint`: passed.
- `scripts/cargo-agent.sh check --lib --manifest-path src-tauri/Cargo.toml`: passed.
- `scripts/cargo-agent.sh tree --manifest-path src-tauri/Cargo.toml -e features -i reqwest@0.12.28`:
  direct Narrant edge resolves `rustls-tls-native-roots` and `rustls_native_certs`.
- Local `gh attestation verify --help`: confirmed `--signer-workflow` grammar is
  `[host/]owner/repo/path/to/workflow`.
- Production Node `--check`: passed for the 11 changed production `.mjs` files checked.
- Ruby Psych: all three changed workflow YAML files parsed.
- `git diff --check 9af8986..f883592`: passed.
- Pure-memory A -> B -> C probe: A and B policies accepted, B archive verified with old key, and the current C
  baseline call using successor failed with exact key-ID mismatch.
- Standards sub-review: PASS. Spec sub-review: NEEDS FIXES with the same Important plus the runbook Minor.

No network, real GitHub API/Release/secret/Environment, trust store, hosts file, native installer, full Rust suite,
or quality-gate operation was used.

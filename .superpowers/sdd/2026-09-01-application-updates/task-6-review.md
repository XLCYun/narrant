# Task 6 independent review

## Verdict

**NEEDS FIXES**

- Critical: 1
- Important: 5
- Minor: 1

固定审查范围：`5d2339285646fa7734d895d6478c0a041855a259..92a8be4fd58d736cc174f132c2a2167ff295a845`。

Draft API 的 fixed-ID、八资产、redirect 去授权、完整 size/SHA/SHA256SUMS/manifest/minisign
复核，evidence 的 exact-two-target/freshness/identity/checksum，以及 promotion 的 fixed-ID
Draft guard/PATCH/Latest 回读总体结构成立。但当前 TLS gate 的 CA 不属于生产 HTTP client 的
trust source，因此真实 normal native smoke 在第一个 manifest TLS 握手就会失败；另有五个会使清理、
attestation、bootstrap 或密钥轮换不满足门禁的 Important。GitHub mutable Release 的 fixed-ID
`guard -> mutation` 无 CAS 微窗口属于明确接受的 residual，本报告不重复列为 finding。

## Critical

### 1. 临时 CA 装进了系统 trust store，但真实 updater client 只信 WebPKI roots

- 位置：`scripts/update-smoke-tls-system-adapter.mjs:87-96`、`:229-237`；实际网络 client：
  `src-tauri/Cargo.toml:31`、`src-tauri/src/application_update/metadata.rs:84-104`、
  `src-tauri/src/application_update/runtime.rs:427-450`。
- 计划 `docs/plans/2026-09-01-application-updates.md:223-226` 和 spec
  `docs/specs/0017-application-updates.md:150-153` 要求真实旧应用在严格 HTTPS 下读取临时 gate。
  gate 只用 `security add-trusted-cert` / `certutil -user -addstore` 安装系统根证书；而仓库直接
  `reqwest 0.12` 依赖启用的是 `rustls-tls`。本机已安装的 reqwest 0.12.28 manifest 明确把该
  alias 映射为 `rustls-tls-webpki-roots`，现有编译 fingerprint 也只有
  `rustls-tls-webpki-roots(-no-provider)`，没有 `rustls-tls-native-roots`。manifest adapter 和
  package downloader 都是这个 direct reqwest client，不是 tauri-plugin-updater 的 reqwest 0.13。
- 因此临时 `github.com` leaf 不可能由内置 WebPKI roots 验证，normal smoke 会在 manifest check
  失败，两个平台都不能产生 normal promotion 所需的 evidence；现有 Node fixture 完全绕过了真实 TLS root
  source，所以 112 个 script tests 无法发现此问题。
- 最小修法：让生产 reqwest 0.12 显式使用系统原生 roots（例如改为/增加
  `rustls-tls-native-roots`，同时保持 HTTPS hostname/redirect 限制），或把 ephemeral CA 注入
  **该 client 实际使用**的 root store。增加一个使用生产 TLS feature/root source 的受控集成证明，
  不能只 mock `systemAdapter`。

## Important

### 1. `--signer-workflow` 传入带 scheme 的无效 identity

- 位置：`.github/workflows/promote-stable.yml:123-130`，尤其 `:127`；静态测试：
  `scripts/tests/update-workflows.test.mjs:45`。
- 本机 `gh 2.68.1 attestation verify --help` 的正式语法是
  `[host/]<owner>/<repo>/<path>/<to>/<workflow>`；当前值为
  `https://github.com/XLCYun/narrant/.github/workflows/update-smoke.yml`，把 `https://` scheme
  放进了 host 段。该参数不能匹配 attestation certificate 的 signer workflow identity，导致受保护
  promotion 在四个文件的 provenance 校验处恒定失败。
- 最小修法：传 `github.com/$GITHUB_REPOSITORY/.github/workflows/update-smoke.yml`，或同 host 下直接
  传 `$GITHUB_REPOSITORY/.github/workflows/update-smoke.yml`；测试需校验完整 grammar，不能只匹配
  `update-smoke.yml` 子串。首次受控 CI 还需保留真实 `gh attestation verify` 证据。

### 2. hosts 已修改但 `routeHostname` 抛错时，finally 不会恢复

- 位置：`scripts/update-smoke-tls-gate.mjs:70-99`、
  `scripts/update-smoke-tls-system-adapter.mjs:251-274`。
- 外层只在 `await routeHostname(...)` 成功返回后才把 `routed=true`。Windows 的
  `writeFile(hostsPath, temporary)` 或 macOS 的 `sudo install` 已经改变 hosts 后若发生部分写、进程
  返回异常或后续步骤抛错，外层会关闭 server/删除 CA，却跳过 `restoreHostname`。独立内存反例让
  adapter 在记录“hosts 已修改”后抛错，实际调用序列为
  `identity, trust, server, hosts-mutated, close, untrust`，没有 `restore`。
- 这违反 plan `:223-224` 和 implementation package `:38-42` 的“every failure restore”契约，且
  现有测试只覆盖 route 成功后 action 失败。最小修法：在进入 route mutation 前就 arm restore，或让
  `routeHostname` 自身在任何 post-backup/post-write 失败中回滚并回读原字节；加入“写后抛错”和
  “恢复回读失败仍保留可重试 backup”的反例，不能在未恢复时删除备份。

### 3. bootstrap 的永久关闭只看当前 Latest，不证明从未存在 updater-capable Stable

- 位置：`scripts/promote-stable-release.mjs:91-97`、`:119-126`。
- implementation package `:49-52` 要求 bootstrap path 不能静默复用，review package 进一步要求证明
  updater-capable public Stable 不存在。当前只检查 `/releases/latest` 是否含 `latest.json`。在 mutable
  Releases 模型下，管理员可以把一个不含 updater manifest 的旧 Release 重新设为 Latest，而先前的
  updater-capable published Release 仍存在；此时只要 protected variable 被误留或重设，bootstrap
  gate 会再次开放。
- 最小修法：在 mutation 前通过有界、分页的 fixed repository API 枚举 published Releases，并永久拒绝
  任一已发布 Release 含 `latest.json`；或使用同等可靠、不可被“换 Latest”绕过的一次性持久化状态。
  加入“current Latest 无 manifest，但历史 published Release 有 manifest”的 promotion 负例。

### 4. bootstrap 的“tampered staging”checkpoint 实际只证明了 stale-version 清理

- 位置：`scripts/native-update-platform-adapter.mjs:321-341`；production 对照：
  `src-tauri/src/application_update/staging.rs:862-883`、`:986-992`。
- fixture 把 metadata `version` 写成正在运行的 candidate version。production recovery 会先在
  `validate_metadata` 以 `version <= current_version` 返回 `NotNewer`，在 `verifier.verify` 之前结束；
  因而 `tampered` package/signature 根本没有被验签路径观察。driver 又只等待 `current` 消失，没有确认
  generation 目录也清理完成，却发布 `bootstrap.tampered_staging_rejected` 和
  `bootstrap.clean_relaunch_observed` checkpoints。
- 这使 plan `:227-228` / spec `:158-159` 的 staging-recovery native proof 比 evidence 声明更弱。
  最小修法：注入一个严格高于运行版本、target/installKind/schema 均有效、但 package/signature 确实
  不匹配的 generation，使 production 进入 verifier；随后确认 pointer 与 generation 均清除，再做正常
  close/clean relaunch。测试需对 fixture 的 exact production rejection branch 作断言。

### 5. runbook 描述的 old-key bridge 在当前发布变量模型中不可执行

- 位置：`docs/application-update-operations.md:142-145`；现有发布边界对照：
  `.github/workflows/release.yml:52-55`、`:245-248`、`:303-325`，以及
  `scripts/release-assets.mjs:436-444`。
- runbook 要求 bridge binary 内嵌 successor public key，同时 bridge updater package 仍由 old private
  key 签名。但发布工作流只提供一个 `NARRANT_UPDATER_PUBLIC_KEY`：它既被编译进 binary，又被
  `prepareDraftAssetDirectory` 用来验证本版 package signature。把它设成 successor 会拒绝 old-key
  签名；保留 old key 则 bridge 仍信 old key，无法建立 successor trust。
- 因此 acceptance criterion 14（spec `:191`）不是一个可操作的 bridge rotation runbook。最小修法：
  设计受保护的 bridge 模式，明确区分“本版 updater package 验签/签名使用的 old key”与“binary
  内嵌的 successor key”，并让 Draft/native smoke/promotion 同时绑定两者；或在文档中明确当前流水线
  不支持 bridge 且先完成该受保护流程，不能把现有步骤写成可直接执行。

## Minor

### 1. 两个 production `Transform` 方法缺少独立中文注释

- 位置：`scripts/prepare-update-smoke-assets.mjs:54-63`、`scripts/public-draft-assets.mjs:206-216`。
- 这两个对象字面量的 `transform(...)` 是生产方法，但只有外层函数注释，没有解释其 byte-bound/hash
  不变量；不符合根 `AGENTS.md:29-31`。最小修法：在方法前补简短中文注释，说明逐 chunk 计数、上限、
  hash 与回传语义。其余 tests 保持在独立 `scripts/tests/`，未发现 Rust inline test 违规。

## Standards axis

- 明确标准 finding：上述 Minor 1。生产脚本的大部分函数已有中文职责/安全边界注释，测试文件分离；
  本范围无前端视觉改动或 Rust 测试布局变化。
- Judgement call：`scripts/native-update-platform-adapter.mjs` 在 state path、process、install、close、
  inspection 和 result contract 多处重复 target switch，存在 possible Repeated Switches；但当前产品明确
  只有两个 target，本轮不把未来扩展成本单列为修复 finding。

## Spec axis

- Draft API fixed release identity、八资产 exact set、手工 redirect 去 token、完整 bytes/size/SHA、
  SHA256SUMS、manifest 和 minisign 交叉验证通过代码与负例审查。
- TLS route/host/method 只允许 shipped latest 和当前 tag/package，candidate package 确实来自已验证 Draft；
  但真实 client trust source 与系统 CA 不相交（Critical 1），并且 post-mutation route failure 不恢复
  hosts（Important 2）。
- normal driver 的 staging path、`current` pointer、schema 2/camelCase fields、target/installKind 与 Task 2/3
  production Rust 对齐；platform install/process/same-path/no-relaunch/manual-launch 结构可由首次 hosted run
  验证。macOS Accessibility 和 Windows installer/registry 的真实 hosted 能力如 report 所述仍是未执行
  residual，本地没有伪称运行成功。
- evidence 文档绑定 source/public/Draft/previous/runner/mode/freshness/checkpoints，目录精确四文件并带
  byte-exact checksum，aggregate 拒绝 duplicate/missing/extra target。bootstrap recovery 与永久关闭仍分别
  有 Important 4、3。
- promotion 的 protected Environment、private `workflow_dispatch`、least permissions、cross-run artifact、
  fixed-ID Draft re-fetch/guard/PATCH/Latest readback成立；attestation CLI identity 参数错误见 Important 1。
- runbook覆盖 secrets/variables、失败/模糊 PATCH 恢复、immutable-disabled、published-asset 禁改、管理员
  concurrency、密钥备份/丢失/泄露；old-key bridge 不可操作见 Important 5。

## Verification performed

- `pnpm test:scripts`：112 passed，0 failed。
- `pnpm lint`：passed。
- 10 个新增 production `.mjs` 的 `node --check`：passed。
- Ruby Psych 解析 `update-smoke.yml`、`promote-stable.yml`：passed。
- `git diff --check 5d23392...92a8be4`：passed。
- `gh version 2.68.1` 与 `gh attestation verify --help`：核对了
  `--signer-workflow [host/]owner/repo/path`、`--source-ref`、`--deny-self-hosted-runners` 的本机语义；
  未访问 GitHub。
- 读取 reqwest 0.12.28 本地 registry feature manifest和现有 compile fingerprint，确认 direct client
  只有 WebPKI roots；未运行 Cargo。
- 执行纯内存 route post-mutation failure 反例，确认未调用 restore；未修改真实 hosts/CA/DNS。
- Node `v26.5.0` 超出仓库 `<26` engine 声明并产生 warning，但上述测试/lint 均成功。
- 未联网、未读取或修改真实 secret/variable/environment/Release，未运行 Cargo，未触发 quality-gate。

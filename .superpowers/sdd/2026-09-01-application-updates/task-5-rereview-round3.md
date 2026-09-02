# Task 5 fix round 3 independent re-review

## Verdict

**APPROVE**

固定复审范围：`99f066970d745e3e93de569bc40d88b2c123f7d4..1fb37f29f7586d29453a959c7777f82f0bead882`。

本轮只复核 `task-5-rereview-round2.md` 的 2 个 Important、1 个 Minor 及 fixed range 新回归。
三项 finding 均已闭环，Standards 与 Spec 两个独立轴都未发现剩余 Critical、Important 或 Minor。
同一 fixed release-ID 上 mutable GitHub Release 的 `guard -> mutation` 无 CAS 微窗口继续按用户决定作为
accepted residual；本轮未重新开启该问题，也未要求或实现 Task 6。

## 上轮 findings 判定

- Important — GFM clickable destination 漏检：**ADDRESSED**。
- Important — response stream/lifecycle 错误可能泄漏 token-bearing cause：**ADDRESSED**。
- Minor — response body 未完整 cancel/release，且 `asset.size` 未与 Blob 大小绑定：**ADDRESSED**。

## Standards axis

结论：**APPROVE**（Critical 0 / Important 0 / Minor 0）。

### Critical

无。

### Important

无。

### Minor

无。

### Standards evidence

- `scripts/release-assets.mjs` 的 GFM parser/遍历和 URL helpers 命名明确、职责内聚；新增方法均有易读
  中文注释。
- `scripts/reconcile-draft-release.mjs` 的 body/reader cancel、lock release、safe rejection 与 size
  identity helpers 均有中文注释；清理层级没有形成需要报告的 Middle Man 或 Duplicated Code。
- 测试仍位于独立的 `scripts/tests/`；`package.json` / lockfile 只是把 release runtime 实际使用的
  `unified`、`remark-parse` 提升为 direct production dependencies。
- 未触及 Rust、前端、Cargo 或 quality-gate，未发现根 `AGENTS.md` 硬违规或可操作的 Fowler smell。

## Spec axis

结论：**APPROVE**（Critical 0 / Important 0 / Minor 0）。

### Critical

无。

### Important

无。

### Minor

无。

### Spec evidence

- `scripts/release-assets.mjs:162-181` 使用锁定的 `remark-parse` + `remark-gfm` 构建 mdast，深度枚举
  `link`、`image` 和 `definition` 的每个 destination；bare-www、bare-email、explicit autolink、
  inline link/image 与 reference-style definition 均进入同一 public-repository HTTPS allowlist。
- `scripts/release-assets.mjs:220-240` 继续独立检查 literal HTTP(S)、HTML URL attributes、
  protocol-relative URL 和非 HTTPS scheme；GFM 解析没有弱化原 raw/HTML 防护。title、notes、manifest
  和每个 asset name 分文档解析，避免跨字段 Markdown 语法拼接。
- `scripts/reconcile-draft-release.mjs:116-249` 在 response metadata 早退、body/header/getReader、
  `reader.read()`、chunk/oversize 和 lock release 失败路径上 best-effort cancel/release；cleanup cause
  被吞掉，既不覆盖预先构造的安全 primary error，也不会把 token-bearing cause/body/header 写入最终错误。
- `scripts/reconcile-draft-release.mjs:266-301` 在发起请求前固定正安全整数 `Blob.size`，并要求响应
  `asset.size` 逐字节相等。该检查只证明本次 request/response consistency，没有冒充 Task 6 的远端
  byte/digest re-fetch。
- 上传仍精确命中 `uploads.github.com/repos/cyunlab/narrant/releases/{releaseId}/assets`；token 只进入
  Authorization header，tag 不会重新解析 mutation target。asset ID/name/state/content-type/API URL/
  browser URL、每次 fixed-ID draft guard、最终 exact-name set 和统一 `basename()` 均未回归。
- `openAsBlob`、global `fetch`、Blob body、Web stream reader/cancel/release 以及新增 parser dependencies
  均覆盖仓库 Node engine 范围；workflow 继续固定 Node 24。未发现 scope creep 或 Task 6 实现。

## Verification performed

- `node --test scripts/tests/release-assets.test.mjs scripts/tests/reconcile-draft-release.test.mjs scripts/tests/release-workflow.test.mjs`：80 passed，0 failed。
- `pnpm test:scripts`：84 passed，0 failed。
- `pnpm lint`：passed。
- 四个 production `.mjs` 的 `node --check`：passed。
- Ruby Psych 解析 `.github/workflows/release.yml`：passed。
- `git diff --check 99f0669..1fb37f2`：passed。
- 新增测试有效覆盖：GFM bare-www/email/reference/image/relative destination；raw/HTML 既有反例；
  metadata 早退 body cancel；header/body/getReader/read/cancel/release 的 token-bearing cause 脱敏；
  primary error 保留；stream oversize；`Blob.size === asset.size` 正反例；fixed release-ID/basename 静态契约。
- Node `v26.5.0` 对仓库 `<26` 约束给出 engine warning；Node 24 兼容性通过受支持 API typings、锁定依赖、
  workflow 配置和当前测试核对，未伪称在 Node 24 二进制上实际运行。
- 未运行 `pnpm install`，避免只读审查修改依赖目录；实现者报告的 offline frozen install 结果仅作为交付
  记录读取，没有替代本轮独立测试。
- 未联网、未读取/修改真实 secrets 或 Releases、未运行 Cargo、未触发 quality-gate、未修改生产代码。

双轴汇总：Standards 轴 0 findings；Spec 轴 0 findings；两轴均为 APPROVE。

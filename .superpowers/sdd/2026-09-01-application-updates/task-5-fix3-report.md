# Task 5 fix round 3 report

## Scope

本轮只修复 `task-5-rereview-round2.md` 的两个 Important 和一个 Minor：最终公开 metadata 的完整
GFM destination 枚举、上传响应流的清理/错误脱敏，以及本地请求字节数与 GitHub asset response 的一致性。
未实现 Task 6 promotion、远端 digest evidence 或 native lifecycle smoke。

实现提交：`25b582a fix(release): validate gfm links and upload lifecycle`。

## GFM destination allowlist

- 将仓库锁中已有的 `unified@11.0.5` 和 `remark-parse@11.0.0` 提升为直接 production 依赖，配合既有
  `remark-gfm@4.0.1` 解析最终 Release Notes/manifest metadata；不再以手写 inline-link 扫描器猜测
  Markdown 语法。
- 深度遍历 mdast，并枚举每个 `link`、`image` 和 `definition` 的 destination。因此 GFM bare-www、
  bare-email autolink、显式 autolink、inline link/image 和 reference-style definition 使用同一 allowlist。
- 所有 GFM destination 解析后必须是 `https://github.com/cyunlab/narrant` 本身或其下级 path；相对路径、
  `mailto:`、GFM 为 bare-www 生成的 `http://`、private/internal host 和其他 GitHub repository 全部拒绝。
- 原有 literal HTTP(S) tokenizer、HTML URL attribute、protocol-relative、非 HTTP scheme、private repository
  identity、Actions URL 和完整 40-hex SHA 棚栏继续保留，形成 parser 语义与 raw/HTML 文本的双重保护。
- GFM 会把紧邻 bare autolink 的 Unicode 标点纳入 AST destination；该类 destination 按 URL 解析后的
  protocol/origin/path 判断，而同一文本的 raw URL 仍执行逐字 canonical 校验，既保留现有中文标点
  fixture，也不放宽 host/repository allowlist。
- 新增 reviewer 三个反例：`www.internal.example/runbook`、`private@example.com`、relative reference
  definition；另覆盖 image definition、relative inline link，以及合法公开 explicit autolink/reference。
  fix2 的 raw/Markdown/HTML fixtures 全部继续通过。

## Upload response lifecycle and redaction

- response metadata validation、256 KiB 限流读取、reader lifecycle 与清理集中在一个深模块内；调用方只会
  收到响应文本或固定中文安全错误。
- redirect、非 2xx、错误 Content-Type、无效/超限 Content-Length、缺少 reader、header/body/getReader
  异常等所有 reader 创建前早退都会 best-effort `body.cancel()`。
- `reader.read()`、chunk lifecycle 或 stream oversize 失败都会 best-effort `reader.cancel()`，并始终尝试
  `releaseLock()`；成功读取后的 `releaseLock()` 异常也会转成固定 lifecycle 错误。
- body/reader cancel 与 lock release 都是 best-effort：cleanup error 不覆盖 primary error。任何 response
  body/header/read/cancel/release cause 或 response body 内容都不会进入最终错误消息。
- fake body/reader 测试逐项记录 body cancel、getReader、read、reader cancel 与 releaseLock 次数，并让
  read/cancel/release 抛出包含测试 token 的错误，证明主错误保留且 token 不外泄。

## Request/response byte identity and basename

- `openAsBlob()` 返回后固定本地 `Blob.size`，要求它是正安全整数，并将其作为本次 upload request 的
  expected bytes。
- GitHub response 的 `asset.size` 必须逐字节等于该 Blob size；不相等即以 asset identity 错误
  fail closed。这只是单次响应一致性，不声称替代 Task 6 的远端 digest/byte re-fetch。
- 单字节成功、错误 size、敏感 size getter cause 均有直接 adapter 测试；size getter 失败时不会发起 fetch。
- 上传前 guard 名称和最终 exact-set 名称现在统一使用 Node `basename()`，删除 path split 重复实现。

## Node and dependency compatibility

- `pnpm install --offline --frozen-lockfile` 通过，证明新增 direct dependency 与现有 lockfile 一致，未访问网络。
- 仓库 `@types/node@22.20.1` 明确定义 `node:fs.openAsBlob`、Web `ReadableStream` reader/cancel/release 和
  Blob/fetch 相关接口；新增 parser 依赖也支持仓库 Node engine 范围。
- release workflow 继续固定 Node 24。当前机器只有 Node `v26.5.0`，因此没有伪称在 Node 22/24 二进制上
  实际运行；通过 Node 22 typings、锁定依赖和 workflow/runtime 静态配置检查兼容性。

## Verification

已执行并通过：

- 聚焦 Node/static tests：80/80。
- `pnpm test:scripts`：84/84。
- `pnpm lint`。
- `node --check scripts/release-assets.mjs`。
- `node --check scripts/reconcile-draft-release.mjs`。
- `node --check scripts/prepare-draft-assets.mjs`。
- `node --check scripts/generate-public-release-notes.mjs`。
- `pnpm install --offline --frozen-lockfile`。
- Ruby Psych `YAML.parse_file(".github/workflows/release.yml")`。
- `git diff --check` 与 implementation staged diff-check。

本机 Node `v26.5.0` 超出仓库声明的 `<26`，pnpm 给出 engine warning；测试、lint、syntax checks 和
offline frozen install 均成功。

按限制未运行 Cargo、未连接 GitHub、未读取或修改真实 secrets/Releases、未触发 quality-gate。

## Residual risks

1. GitHub mutable Release API 没有能覆盖“读取 Draft 状态 + 单次 mutation”的可靠 CAS，且项目明确不启用
   immutable releases。每个 DELETE、PATCH、upload 前后虽按固定 release ID 核对 `id/tag/draft`，管理员
   仍可能在 guard 返回后、mutation 到达 GitHub 前并发发布；不能声称绝对 CAS 或原子 reconciliation。
   Task 6 protected promotion 必须再次核对 Draft identity、bytes/digests 与 evidence。
2. 多资产 Draft reconciliation 仍不是事务；中途失败可能留下 partial Draft，但同 tag 重试会按固定 ID
   清理并完整重建，且不会主动把 published Release 重新设为 Draft。
3. 真实 Developer-ID/notary/stapler、Tauri signer、Windows NSIS 签名和 GitHub uploads endpoint 仍需私有
   tag workflow 在原生 runner 与真实 secrets 下验证；本轮没有发起网络或伪称执行这些远端步骤。
4. `Blob.size === asset.size` 只校验本次 upload response 的字节数身份；远端资产 digest/bytes 的二次证明、
   native lifecycle smoke 与最终 promotion 仍属于 Task 6。

# Task 5 fix round 2 report

## Scope

本轮只修复 `task-5-rereview-round1.md` 剩余的三个 Important，不包含 Task 6 promotion，未连接或修改
任何真实 GitHub Release、secret、variable 或公开仓库。

实现提交：`134c4c2 fix(release): bind draft uploads to release id`。

## Minisign inner contract

- updater 公钥和签名的 outer base64 继续要求 standard、带 padding、无空白且 round-trip canonical。
- outer 解码后的文本不再经过 `.trim()` 或 CRLF 归一化：公钥必须精确两行，签名必须精确四行，
  不接受 CR、尾随换行或任意行尾空白。
- inner public-key packet、signature packet 和 global signature 均再次执行 canonical base64 校验。
- 发布验签只接受 prehashed `ED`：先对包计算 BLAKE2b-512，再以 Ed25519 验证 package signature；
  legacy `Ed` 不在发布 validator 的接受集合内。
- 测试使用真实本地 Ed25519 fixture 生成 `ED` 正例，并覆盖 canonical outer 包裹 inner 末尾空格、
  尾随换行、CRLF、comment 尾随空白、公钥尾随空白、legacy `Ed`、outer 尾随换行和缺 padding 等反例。
  `.sig` 文件仍要求自身是无换行的 canonical outer base64，manifest 逐字等于该文件。

## Public metadata URL policy

- 每一个 `http://` / `https://` scheme 都独立切词；逗号、分号、括号、Markdown 标点、JSON 转义边界和
  相邻 scheme 不会再把后一个 private URL 吞进公开 URL 的 path。
- 单独提取并验证 Markdown inline destination 和 HTML URL attributes；protocol-relative destination、
  `mailto:`、`data:`、`javascript:`、`file:`、`ftp:`、`ssh:`、`git:`、`tel:`、`sms:` 及其他
  `scheme://` 均 fail closed。
- 唯一 allowlist 是 canonical `https://github.com/cyunlab/narrant` 本身或其下级 path；继续拒绝凭据、
  端口、HTTP、其他 GitHub 仓库、internal/private host 和 `/actions/`。
- 保留普通 conventional commit subject（例如 `fix:`），避免把没有 link destination 语义的冒号文本
  误判成 URI。
- fixtures 覆盖逗号、分号、无分隔相邻 URL、Markdown/HTML private link、protocol-relative host、
  localhost、非 HTTP scheme、malformed URL，以及 raw/Markdown/HTML 三种合法公开仓库 URL。

## Fixed release-ID upload adapter

- 移除 `gh release upload <tag>` / `--clobber`，也不依赖 `gh api --hostname uploads.github.com` 的
  host/path 重写语义。
- Node adapter 严格构造
  `https://uploads.github.com/repos/cyunlab/narrant/releases/{releaseId}/assets?name={encodedName}`，
  每个资产均在上传前后按同一 release ID 回读并核对 `id/tag/draft`；全部上传后再次按 ID 回读 exact set。
- `GH_TOKEN` 只进入 `Authorization: Bearer ...` header，不进入 URL、argv、错误正文或日志。请求使用
  `redirect: "error"` 和 `application/octet-stream`。
- Node 24 `openAsBlob` 让 DMG/EXE 请求体由文件 Blob 提供，不把整个大文件一次性读入 JavaScript heap。
- 上传响应要求 2xx、非 redirect、JSON content type；`Content-Length` 和实际 stream 都受 256 KiB
  上限约束，并严格验证 UTF-8、JSON、正整数 asset ID/size、名称、state、content type、exact
  `api.github.com` asset URL 与 exact public download URL。
- adapter 可注入 `fetch` 和 `openBlob`。纯内存测试覆盖固定 endpoint/文件名编码、binary Blob、token
  缺失与脱敏、文件读取失败、network、redirect、非 2xx、错误 content type、无效/超限
  Content-Length、stream 超限、无效 UTF-8/JSON 和 response identity 偏差；未发起网络请求。
- 交错测试证明 upload guard 前的 identity drift 会在 mutation 前停止；guard 返回后的 retag 仍只向固定
  release ID port 发起一次上传，随后立即按该 ID 回读并 fail closed，不会按 tag 重解析新对象。

## Verification

已执行并通过：

- 聚焦 Node/static tests：64/64。
- `pnpm test:scripts`：68/68。
- `pnpm lint`。
- `node --check scripts/release-assets.mjs`。
- `node --check scripts/reconcile-draft-release.mjs`。
- `node --check scripts/prepare-draft-assets.mjs`。
- `node --check scripts/generate-public-release-notes.mjs`。
- Ruby Psych `YAML.parse_file(".github/workflows/release.yml")`。
- `git diff --check` 与 implementation staged diff-check。

本机 Node `v26.5.0` 超出仓库声明的 `<26`，pnpm 给出 engine warning；上述测试与 lint 均成功，
release workflow 固定使用受支持的 Node 24。

按限制未运行 Cargo、未连接 GitHub、未使用真实签名/公证凭据、未触发 quality-gate。

## Residual risks

1. GitHub mutable Release API 没有能覆盖“读取 Draft 状态 + 单次 mutation”的可靠 CAS，而且项目明确不启用
   immutable releases。当前代码在每个 DELETE、PATCH、upload 前后都按固定 release ID 核对
   `id/tag/draft`，但管理员仍可能在 guard 返回后、对应 mutation 到达 GitHub 前并发发布；这是无法消除的
   微窗口，不能声称绝对 CAS 或原子 reconciliation。Task 6 protected promotion 必须再次核对 Draft identity、
   bytes/digests 与 evidence，作为最终发布二次门禁。
2. 多资产 Draft reconciliation 本身不是事务；中途失败会留下 partial Draft，但同 tag 重试会先按 ID 删除
   旧资产再完整重建，且不会覆盖已发布 Release。
3. 真实 Developer-ID/notary/stapler、Tauri signer、Windows NSIS 签名和 GitHub uploads endpoint 仍需私有
   tag workflow 在原生 runner 与真实 secrets 下验证；本轮没有伪称执行过这些远端步骤。
4. 上传后资产 bytes/digests 的 API 二次证明和 native lifecycle smoke 仍属于 Task 6，本轮只保留本地预验、
   单资产响应 identity 与远端 exact-name set。

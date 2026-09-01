# Task 5 fix round 2 independent re-review

## Verdict

**NEEDS FIXES**

固定复审范围：`d401eedf24cbc4b86a1bdf33d2c8c9abaa8d91a6..94d02c63a0dc85f7baf8722598d5ee8b2c26c31c`。

上轮 minisign inner-text contract 与 tag-retargeting 两项已修复；公开链接策略仍漏掉 GitHub
Flavored Markdown 会生成的链接。新 fixed release-ID 上传适配器的 endpoint、Node 24 API、响应上限、
redirect/status/identity 基本验证及常规 token 脱敏方向正确，但流读取异常的错误脱敏和失败响应体释放
尚未闭合。已记录的同一 fixed release-ID 上 mutable Release `guard -> mutation` 无 CAS 微窗口仍按用户
决定作为 accepted residual；本轮没有把它误判为已消除的 tag retargeting，也未要求实现 Task 6。

## 上轮三个 Important 的状态

- decoded minisign public-key/signature text：**ADDRESSED**。发布侧不再 `.trim()` 或 CRLF 归一化，
  canonical outer 包装 inner 前导/尾随空白、尾随换行和 CRLF 的反例均被拒绝；接受集合不大于当前
  Rust `PublicKey::decode` / `Signature::decode` + `verify_stream` 客户端路径。
- every clickable/literal public URL destination：**PARTIALLY ADDRESSED**。delimiter 拼接、inline
  Markdown、HTML attributes、protocol-relative 和显式非 HTTPS scheme 已覆盖；GFM autolink 与
  reference definition 仍漏检，见两轴 Important。
- upload fixed release-ID：**ADDRESSED**。不再调用 `gh release upload <tag> --clobber`，二进制请求
  固定命中 `uploads.github.com/repos/cyunlab/narrant/releases/{releaseId}/assets`；guard 后 retag 不会
  改变 mutation target，且每个上传前后继续按固定 ID 核对 tag/draft。

## Standards axis

结论：**NEEDS FIXES**（Critical 0 / Important 1 / Minor 1；另有 1 个 Fowler judgement call）。

### Critical

无。

### Important

1. **公开元数据仍会接受 GFM 自动生成的私有链接。**
   `scripts/release-assets.mjs:142-231` 只收集字面 HTTP(S) scheme、inline Markdown `](`、HTML URL
   attributes、protocol-relative 和显式 URI scheme。`www.internal.example/runbook` 与
   `private@example.com` 都不会进入候选集合，但 GFM 会分别生成 `http://www.internal.example/...`
   与 `mailto:private@example.com` 的可点击链接。独立本地探针证明 validator 接受两者，而仓库已有的
   `remark-gfm` 解析器将它们解析为 `link` node。最小修复是用 GFM parser 校验最终 metadata 中每个
   link/definition destination，或显式拒绝 GFM bare-www/email autolink，并增加负例。

### Minor

1. **上传失败响应没有完整释放 body。**
   `scripts/reconcile-draft-release.mjs:118-148,197-206` 在 redirected、非 2xx、错误 Content-Type、
   非法/超限 Content-Length 时直接抛错而不 cancel response body；`reader.read()` 异常也只
   `releaseLock()`。内存 lifecycle 探针对 HTTP 422、错误 Content-Type 和 declared oversize 均得到
   `cancel-count 0`。单次 Actions CLI 的影响有限，但真实 403/422 等失败会留下未消费流/连接。
   所有早退及读取异常应 best-effort cancel，并以 fake stream 断言 cancel/lock 生命周期。

### Standards / smells

- 未发现根 `AGENTS.md` 的硬违反：新增方法均有中文注释；测试保持在独立 `scripts/tests/`；无 Rust、
  前端、Cargo 或 quality-gate 触点。
- Fowler judgement call：`reconcileDraftRelease` 在 `:296` 使用 `basename()`，最终 exact-set 在 `:306`
  又用路径 split 提取同一资产名，属于轻微 Duplicated Code；建议统一调用 `basename()`，不单独升档。

## Spec axis

结论：**NEEDS FIXES**（Critical 0 / Important 2 / Minor 1）。

### Critical

无。

### Important

1. **“every clickable or literal URL/destination”仍未实现。**
   除 bare `www` 与 email autolink 外，reference-style Markdown 也能绕过：
   `[内部][ref]\n\n[ref]: /private/runbook` 被 validator 接受，但 GFM 产生 URL 为
   `/private/runbook` 的 definition/link；在 GitHub Release 页面会成为 allowlist 外的可点击目标。
   这违反 `task-5-fix2-review-package.md` 的全部 clickable/literal destination 要求及 plan Task 5
   requirement 5 的 private-link 边界。应解析最终 GFM AST，而不是继续扩展易漏语法的正则集合。

2. **response stream 异常仍可未经脱敏进入最终错误输出。**
   `scripts/reconcile-draft-release.mjs:137-148,208` 没有把 `reader.read()`、`reader.cancel()` 或 reader
   lifecycle 异常转换成固定安全错误；`main` 又会在 `:362-365` 原样写入最终 message。纯内存探针让
   response reader 抛出 `Error(GH_TOKEN)`，顶层错误逐字包含 token。真实 Node/Undici 通常只产生通用
   transport error，本次没有声称通过真实网络证明 token echo；但实现没有兑现 review package 明确的
   “token 不进入 error output”不变量。最小修复是在 response read/cancel/lifecycle 边界捕获并替换为
   不包含 cause/body/header 的固定错误，并加入 token-bearing stream-error 负例。

### Minor

1. **上传响应一致性尚不完整。**
   `parseUploadedAsset` 在 `scripts/reconcile-draft-release.mjs:91-114` 只要求 `asset.size > 0`，未将它
   与已知 request Blob 的 `body.size` 比较；1-byte Blob 配合 `asset.size = 999` 的响应会被接受。
   这只是单次 upload response consistency，不要求越界执行 Task 6 的远端 bytes/digest re-fetch；将
   expected size 传入 identity validator 即可。同时按 Standards Minor 所述补齐 response body cancel。

### Spec compliance notes

- `openAsBlob`、global `fetch`、Blob request body 和 Web-stream response API 均在仓库支持的 Node 22
  typings 中存在，因此 workflow 的 Node 24 运行时兼容性未见问题；Blob body 不需要把 DMG/EXE 整体
  读入 JS heap。
- endpoint 精确固定到 public repository + release ID，name 经过 `encodeURIComponent`；token 仅进入
  Authorization header，不进入 endpoint/argv，fetch/open-file/HTTP/JSON/identity 常规错误也使用安全
  文本。redirect 使用 `error`，成功响应与真实 stream 都受 256 KiB 上限控制。
- asset ID/name/state/content-type/API URL/browser download URL 与最终 fixed-ID Draft exact-name set 均有
  fail-closed 校验；未发现 tag 被重新解析或 `--clobber` 回归。
- 未发现 scope creep；没有实现 Task 6 promotion、remote byte/digest evidence 或 native smoke。

## Verification performed

- `node --test scripts/tests/release-assets.test.mjs scripts/tests/reconcile-draft-release.test.mjs scripts/tests/release-workflow.test.mjs`：64 passed，0 failed。
- `pnpm test:scripts`：68 passed，0 failed。
- `pnpm lint`：passed。
- 四个 production `.mjs` 的 `node --check`：passed。
- Ruby Psych 解析 `.github/workflows/release.yml`：passed。
- `git diff --check d401eedf..94d02c6`：passed。
- 纯内存负向探针证明：GFM bare-www/email/reference destination 被 validator 接受；stream error 可原样
  带出测试 token；HTTP/content-type/declared-length 早退不 cancel body；1-byte Blob 的 size=999
  response 被接受。探针没有联网或修改文件/Release。
- Node `v26.5.0` 对仓库 `<26` 约束给出 engine warning；Node 24 兼容性以受支持 API/typings 和 workflow
  静态配置核对，未伪称在 Node 24 二进制上实际运行。
- 未连接 GitHub、未读取/修改真实 secrets 或 Releases、未运行 Cargo、未触发 quality-gate。

双轴汇总：Standards 轴 1 Important + 1 Minor（最严重为 GFM private-link bypass）；Spec 轴
2 Important + 1 Minor（最严重为 clickable destination 漏检与 response-stream error 脱敏缺口）。

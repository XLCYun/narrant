# Task 5 fix round 1 report

## Outcome

已修复 `task-5-review.md` 的全部 Critical、Important 和 Minor。实现提交为 `b847f1a`
（`fix(release): harden signed draft reconciliation`）。本轮没有实现 Task 6、触发远端 workflow、
访问 GitHub Release、使用生产签名凭据或运行 Cargo/quality-gate。

## Review findings resolved

### DMG installer layout and final verification

- workflow 不再对 `Narrant.app` 本身使用 `hdiutil create -srcfolder`。
- 使用明确的临时 staging root，把最终已 Developer-ID 签名、公证并 stapled 的 app 复制为卷根
  `Narrant.app`，并加入指向 `/Applications` 的 symlink；DMG 从 staging root 创建。
- 最终已签名、公证并 stapled 的 DMG 以 `-readonly -nobrowse` 回挂到明确 mount point。
- EXIT trap 在后续任何布局或盘内验证失败时都尝试普通 detach，再 fail-safe force detach。
- 回挂后证明卷根只有唯一 `.app` 且名称为 `Narrant.app`，验证 `/Applications` symlink，并直接对
  盘内 app 运行 `codesign --verify --deep --strict`、`stapler validate` 与 `spctl --type execute`。
- workflow 静态测试固定 staging、mount、布局、fail-safe detach 和最终盘内验证的顺序，并明确拒绝
  `-srcfolder "$app_path"` 回归。

### Client-compatible updater signature contract

- 发布 validator 只接受 prehashed `ED` updater signature，不再接受客户端
  `DiskMinisignVerifier::verify_stream` 明确拒绝的 legacy `Ed`。
- 包签名输入固定为 BLAKE2b-512 digest，再用 Ed25519 验证；可信注释的全局签名继续额外验证，因此
  发布侧接受集合不大于客户端 verifier。
- public key、`.sig` outer-base64 和内部 minisign packet 都要求 standard canonical base64：无空白、
  无尾随换行、需要时必须带 padding，并要求逐字节 round-trip；outer 文本还必须是有效 UTF-8。
- `.sig` 文件必须本身就是 canonical 单行文本；`latest.json.signature` 与文件字面内容完全相等。
- 测试 fixture 改为运行时生成 canonical `ED`：先 BLAKE2b-512，再 Ed25519 sign；新增 legacy `Ed`、
  尾随空白、缺 padding、错误 canonical 公钥、错误 key 和包篡改反例。

### Public metadata privacy

- 任意独立的 40-hex Git SHA 都被拒绝，不再只比较当前 `GITHUB_SHA`。
- 从 title、notes、manifest 和 asset names 抽取所有 `http(s)` URL；唯一 allowlist 是无凭据、无自定义
  端口的 `https://github.com/cyunlab/narrant[/...]`。
- 即使位于公开仓库路径，Actions URL 仍被拒绝；其他 GitHub private repo、internal host、HTTP 和
  带身份信息 URL 都 fail closed。
- 继续拒绝私有仓库 identity；fixtures 覆盖其他 SHA、private issue、internal URL、HTTP、Actions
  URL 以及允许的公开仓库链接。

### Strongest available published-release guard

- tag lookup 后固定 release ID；后续 guard 全部通过 `repos/.../releases/{id}` 回读并同时核对
  `id/tag/draft`。
- 每个 DELETE 前后、metadata PATCH 前后、upload 前后都紧邻复核 Draft identity。
- create 成功后先按 tag 回读 identity，再按 release ID 回读；只有两次都仍为同一 Draft 才允许上传。
- metadata edit 改为 release-ID PATCH，且请求中没有 `--draft`、`draft=true` 或任何重新降回 Draft 的
  字段。
- 测试在 delete、edit、upload guard 各边界切换 published，证明对应 mutation 尚未执行即停止；
  另覆盖每类 mutation 后 published 回读、create 后 published、ID/tag 漂移和最终 exact set。

### `gh` child-process adapter and exact names

- child process 的 `exit` 不再 settle promise；stdout/stderr pipes 关闭并发出 `close` 后才一次性返回。
- `error` 只记录错误并等待 `close`，一次性 guard 防止重复 settle；spawn 可注入 fake child。
- lifecycle 测试在 `exit` 后继续追加 stdout/stderr，证明 `close` 前不返回且最终字节完整；另覆盖
  `error` 后 pipe drain 与重复 close。
- duplicate/missing/unexpected name 诊断提取为 `assertExactNameSet`，资产准备和 Draft 回读共用同一
  helper，删除重复实现。

## Verification

已执行并通过：

- 聚焦 Node/static tests：32/32。
- `pnpm test:scripts`：36/36。
- `pnpm lint`。
- `node --check scripts/release-assets.mjs`。
- `node --check scripts/reconcile-draft-release.mjs`。
- `node --check scripts/prepare-draft-assets.mjs`。
- `node --check scripts/generate-public-release-notes.mjs`。
- Ruby Psych `YAML.parse_file(".github/workflows/release.yml")`。
- `git diff --check` 与 implementation staged diff-check。
- 本机无凭据最小 `hdiutil` fixture：从 staging root 创建 DMG，只读挂载后确认唯一
  `Narrant.app` 和 `/Applications` symlink，随后安全 detach 并清理临时目录。该 fixture 只证明
  DMG 布局/挂载语义，不代表真实 app 签名、公证或 Gatekeeper 验收。

本机 Node `v26.5.0` 超出仓库声明的 `<26`，pnpm 输出 engine warning；全部测试与 lint 仍成功，
Actions workflow 固定使用受支持的 Node 24。

## Residual risks

1. GitHub mutable Release API 不提供能覆盖“读取 Draft 状态 + 删除/编辑/上传”的可靠 CAS 或跨调用
   事务，而且本项目明确不启用 immutable releases。当前实现已在每次 mutation 前后按 release ID
   做最强 fail-closed guard，但管理员仍可能在某次 guard 返回后、对应 mutation 到达 GitHub 前的
   微窗口并发发布；不能把这描述为绝对 CAS 保证。Task 6 protected promotion 必须再次回读 Draft
   identity、bytes/digests 和 evidence，作为最终发布二次门禁。
2. 真实 Developer-ID/notary/stapler、Tauri signer 与 Windows NSIS `.exe.sig` 仍需私有仓库 tag
   workflow 在原生 runner 和真实 secrets 下验证；本轮没有伪称运行过这些远端步骤。
3. 上传后资产 byte/digest 的 API 二次回读和 native lifecycle smoke 仍属于 Task 6；本轮保持 Task 5
   只证明本地预验与远端 exact-name set。

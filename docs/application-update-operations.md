# Application Update 发布运维

Narrant 的源码与发布控制位于私有仓库 `XLCYun/narrant`，公开安装包与 Stable updater
Release 位于 `cyunlab/narrant`。公开仓库不是源码镜像。客户端唯一 Stable endpoint 是：

```text
https://github.com/cyunlab/narrant/releases/latest/download/latest.json
```

每次发布先在公开仓库形成完整 Draft；两平台真实原生 smoke 通过后，维护者审批私有仓库
`stable-promotion` Environment 中的 job，自动化才把同一个固定 Release ID 发布并标为 Latest。
不得用公开仓库的 `release.published` event 反向触发私有发布控制。

## GitHub 配置

私有仓库需要以下 Actions secrets：

- `CYUNLAB_RELEASE_TOKEN`：fine-grained token，只授权 `cyunlab/narrant` 所需的 Release/
  Contents 读写。它用于创建/回读 Draft 和最终发布，不得授权私有源码仓库之外的额外仓库。
- `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：Narrant updater
  minisign 私钥与密码，仅供发布构建使用。
- `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、
  `APPLE_API_ISSUER`、`APPLE_API_KEY`、`APPLE_API_PRIVATE_KEY`：macOS Developer ID
  签名、公证与 stapling。

私有仓库需要以下 Actions variables：

- `NARRANT_UPDATER_PUBLIC_KEY`：与发布私钥匹配、并编译进应用的 Tauri outer-base64
  minisign 公钥。
- `NARRANT_GITHUB_CLIENT_ID`、`NARRANT_GITHUB_APP_SLUG`：现有 GitHub Identity 构建配置。

创建名为 `stable-promotion` 的 protected Environment：

1. 配置 required reviewers，至少一名不负责本次构建的维护者批准；禁止 self-review（如果组织策略支持）。
2. 将 `CYUNLAB_RELEASE_TOKEN` 和 `NARRANT_UPDATER_PUBLIC_KEY` 放在该 Environment，或用
   同名仓库配置并确保 Environment 审批仍是 job 的必经门。推荐把发布 token 放在 Environment。
3. 首个 updater-capable 版本期间，设置 Environment variable
   `NARRANT_FIRST_UPDATER_TAG=vMAJOR.MINOR.PATCH`。第一次成功后删除该变量。即使变量被误留，
   promotion 也会在发现已有包含 `latest.json` 的 published Stable 时永久拒绝 bootstrap。
4. Environment deployment branch policy 只允许默认分支。workflow 始终 checkout 默认分支上的
   可信脚本，再把 source tag/SHA 当作不可信数据验证，不执行 tag 中的脚本。

`update-smoke.yml` 需要读取 Draft，因此 runner 也使用 `CYUNLAB_RELEASE_TOKEN`。脚本只执行
Release GET/asset download；token 只进入 `Authorization` header，不进入 URL、argv、artifact、
evidence 或错误正文。最终写操作只存在于受保护的 promotion job。

## 正常发布流程

1. 在私有仓库创建并推送经批准的 `vMAJOR.MINOR.PATCH` tag。确认 tag 解引用为默认分支上的
   exact 40 位 lowercase commit SHA。
2. 等待 `release.yml` 完成。它构建 macOS arm64 和 Windows x64，签名 updater packages，向
   `cyunlab/narrant` 创建或完整替换一个 Draft。Draft 必须恰好含八个资产：两个安装包、两个
   updater packages、两个 `.sig`、`latest.json` 和 `SHA256SUMS`。
3. 从 release job 日志取得固定 public Release database ID，或用 token 做只读查询：

   ```bash
   gh api repos/cyunlab/narrant/releases/tags/vMAJOR.MINOR.PATCH --jq .id
   ```

   同时记录 private source tag/full SHA 与 public Release ID/tag；public tag 不能代替 private SHA。
4. 记录当前 published Stable 的 tag，并下载当前固定 endpoint 的 `latest.json`，对完整 bytes
   计算 lowercase SHA-256。不要从 Draft body、GitHub “latest” 网页或本地重建猜 previous identity。
5. 在私有仓库手动运行 `Native application update smoke evidence`：

   - `mode=normal`；
   - 传入 source tag/full SHA、fixed public Release ID/public tag；
   - 传入 previous Stable tag 与刚记录的 manifest SHA-256。

   workflow 会在 `macos-15` arm64 和 Windows x64 上重新通过 API 下载 Draft 的八个资产，校验
   asset ID/size/完整 SHA-256、`SHA256SUMS`、manifest 和 minisign，再安装真实 previous Stable。
   runner 临时 CA 只信任本次 `github.com` leaf，hosts 只增加 loopback mapping；gate 只服务固定
   latest 路径和当前 tag/target package。无论成功失败，hosts 按原字节恢复、DNS 刷新、helper
   退出、临时 CA 删除。任何 cleanup 失败保留原 native 根因。
6. 两个平台必须观察真实 check/download/durable staging、原生窗口正常关闭、共享 stop 完成后的
   安装、同路径替换、无自动 relaunch，以及 harness 手工启动候选版本。aggregate job 只接受
   exactly `darwin-aarch64` 和 `windows-x86_64` 两份 fresh `real-native` evidence；每份 JSON 都有
   byte-exact `.sha256` companion，并由 GitHub artifact attestation 绑定到私有 workflow identity。
7. 记下成功 smoke workflow run ID。手动运行 `Promote public Stable release`，输入完全相同的
   source/public/previous identity 和 evidence run ID。等待 `stable-promotion` Environment 审批。
8. 审批者先核对 source tag/SHA、public Draft ID/tag、Release Notes、previous Stable 和 smoke run。
   批准后，job 校验 evidence artifact attestation，重新通过固定 public Release ID 下载和验签八个
   Draft assets，重验 freshness/两目标/previous identity，并在 mutation 紧前再次校验 Draft ID、tag、
   asset ID/size/state。最后只 PATCH 该 Release 为 `draft=false`、`prerelease=false`、Latest，并回读
   `/releases/latest` 确认固定 ID。
9. 从一台不带 Actions 凭据的机器匿名读取 Stable endpoint，确认 version、两 target URL 和签名。
   不要在已发布 Release 上“修补”资产；任何内容变化都发布更高 SemVer。

## 一次性 first-updater bootstrap

v1.1.5 没有 updater，不能伪造 v1.1.5→候选的自动升级证据。首个 updater-capable 版本采用一次性
bootstrap：

1. 在 `stable-promotion` Environment 设置 exact `NARRANT_FIRST_UPDATER_TAG`。
2. 正常生成该 tag 的完整 Draft。
3. 运行 smoke 时选择 `mode=bootstrap`，previous Stable 两项必须留空。两平台证据只声明：签名安装包
   安装、exact installed candidate 启动、内嵌 endpoint/public key、候选版本、损坏 staging 被恢复清除、
   正常关闭后 clean relaunch；它不声明旧版本自动升级。
4. promotion 也选择 `mode=bootstrap`。protected variable 必须与 source tag 完全相同，而且当前公开
   Latest 不得已有 `latest.json`。任一条件不满足都在发布 mutation 前失败。
5. 第一次成功后删除 variable 并永久使用 `mode=normal`。保存 bootstrap workflow run、两份 evidence、
   checksums、attestation 和 Release ID 作为一次性审计记录。不得因重跑方便而复用 bootstrap。

## 失败与恢复

### Draft 或 smoke 失败

- 失败发生在 promotion PATCH 前时，公开 Stable 不变。保留 Draft，查明原因后可对同一个 Draft 重跑
  `release.yml` 的完整 reconciliation；脚本会拒绝任何已发布 Release。
- 部分 Draft、重复/额外资产、asset ID/size/bytes 漂移、错误 signature/manifest/checksum 均不得人工
  绕过。重新生成完整八资产，不要只补单个文件后继续旧 smoke evidence。
- smoke runner 失败后确认日志显示 hosts、CA 和 helper cleanup 已执行。若 cleanup 也失败，废弃该 runner；
  不得把该次运行的 evidence 用于 promotion。
- evidence 缺失、重复、超过 24 小时、checksum 变化、attestation 失败、source/public/previous identity
  不匹配时重新运行完整两平台 smoke，不能手工编辑 evidence 或 checksum。

### Promotion 失败或并发

- GitHub mutable Releases 没有可供本流程使用的跨“guard→mutation”原子 compare-and-swap。项目明确**不启用
  GitHub immutable releases**。自动化使用固定 Release ID、mutation 紧前 guard 和 mutation 后 Latest 回读，
  但 release 管理员仍必须在 smoke/promotion 期间避免并发发布、改 tag 或改 Draft assets。
- 如果 promotion 在 PATCH 前失败，Draft 保持未发布。复核 Draft bytes 和 evidence freshness 后重跑。
- 如果网络在 PATCH 后中断，先只读查询 fixed Release ID 和 `/releases/latest`；不要直接重跑或创建同 tag
  Release。若已发布，按已发布失败处理并保留审计记录。
- 自动化**永远不编辑 published Release 的资产或 Release Notes**。发布后发现问题时停止进一步推广，保留
  原 Release 供审计，构建一个更高 SemVer 修复版。客户端不自动降级。

### Token 泄露

立刻暂停 `release`、`update-smoke` 和 `stable-promotion`，撤销 `CYUNLAB_RELEASE_TOKEN`，审计
`cyunlab/narrant` 的 Release/tag/asset 变化，再创建最小权限替代 token。任何未经确认的 Draft bytes
都必须完整重建和重跑两平台 evidence。不要把 token 或 GitHub redirect signed URL贴入 issue。

## Updater 私钥备份、丢失与轮换

- updater 私钥是已安装客户端的长期信任根。至少保留一份离线加密备份；私钥密文与解密材料分开保存，
  分别限制访问并记录责任人。定期在隔离环境恢复副本、签名 disposable package，再用对应 public key
  验证；不得把恢复材料上传到 Actions、Release、cache、artifact、日志或支持包。
- **私钥丢失**：旧客户端不会接受新密钥签名。停止发布，恢复经验证的离线备份。无法恢复时不能仅替换
  Actions secret；需要用户手动安装重新建立信任的新版本，并进行安全/产品评审。
- **疑似私钥泄露**：立刻停止 promotion、撤销相关 CI 访问并保存审计记录。攻击者若拥有旧私钥，也能签
  bridge，因此不能把普通自动轮换当作充分处置；先完成事件响应和独立安全评审，必要时要求手动安装。
- **计划轮换**：在旧私钥仍可信时，先发布一个由旧钥签名的 bridge release。bridge 把 successor public
  key 编译进客户端，同时仍可由旧客户端验证。bridge 按 normal 两平台真实升级流程发布并达到 Stable；
  确认采用后，才把构建 secret 切换到 successor private key，更新 `NARRANT_UPDATER_PUBLIC_KEY`，并发布
  由新钥签名的下一版。保留旧 key 的离线退役记录，不得让同一版本混用两把钥匙。

## 每次发布检查表

- [ ] private tag/full SHA 属于默认分支；版本在 package、Tauri、Cargo 中一致。
- [ ] public Draft ID/tag 已单独记录，八资产集合、Release Notes 和签名完整。
- [ ] macOS Developer ID/codesign/Gatekeeper/notarization/stapling 全部通过。
- [ ] Windows 是 x64 current-user NSIS；首期不要求 Authenticode，但 updater minisign 必须通过。
- [ ] normal evidence 绑定真实 previous Stable manifest；bootstrap 只在一次性首版使用。
- [ ] exactly 两份 fresh real-native evidence、checksums 和 GitHub attestation 均通过。
- [ ] `stable-promotion` required reviewer 已核对身份并批准。
- [ ] promotion 二次回读 fixed Draft 后才发布；published Release 未被自动化编辑。
- [ ] 匿名 Stable endpoint 已复核；发布记录与 evidence run ID 已归档。
- [ ] 管理员理解 immutable releases 未启用，并确认整个窗口没有并发 Draft/Release 修改。

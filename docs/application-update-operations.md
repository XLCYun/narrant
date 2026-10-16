# Application Update 发布运维

Narrant 的源码与发布控制位于私有仓库 `XLCYun/narrant`，公开 GitHub Release 位于
`cyunlab/narrant`，作为公开制品归档而不是源码镜像。客户端和官网唯一 Stable endpoint 是：

```text
https://updates.cyunlab.com/narrant/channels/stable/latest.json
```

每次发布先在公开仓库形成完整 Draft；私有 `release.yml` 随后自动复用两平台真实原生 smoke，
通过后维护者手动 dispatch 私有仓库
`stable-promotion` Environment 中的 job，自动化才把同一个固定 Release ID 发布并标为 Latest。
不得用公开仓库的 `release.published` event 反向触发私有发布控制。若 GitHub 计划支持 required
reviewers，还应在该 Environment 增加独立审批；当前 user-owned private repository 的 GitHub Free
计划不提供此能力。

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

- `NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY`：只在 normal smoke/promotion 中验证 published previous
  Stable archive 的公钥。它描述历史包的 signer，可能在 bridge 后暂时不同于当前 release key；bootstrap
  必须不配置该值。
- `NARRANT_RELEASE_SIGNING_PUBLIC_KEY`：验证本版 updater package 签名的 Tauri
  outer-base64 minisign 公钥；正常情况下与发布私钥及下项匹配。
- `NARRANT_UPDATER_PUBLIC_KEY`：编译进候选应用、用于验证下一版 updater 的公钥。普通发布必须与
  `NARRANT_RELEASE_SIGNING_PUBLIC_KEY` 相同；只有计划轮换的 bridge tag 可以不同。
- `NARRANT_UPDATER_BRIDGE_TAG`：通常留空。只在计划轮换时设置为唯一 exact
  `vMAJOR.MINOR.PATCH` bridge tag；它不是通配符或长期开关。
- `NARRANT_GITHUB_CLIENT_ID`、`NARRANT_GITHUB_APP_SLUG`：现有 GitHub Identity 构建配置。

创建名为 `stable-promotion` 的 protected Environment：

1. deployment branch policy 只允许默认分支。若 GitHub 计划支持，配置 required reviewers，至少一名
   不负责本次构建的维护者批准并禁止 self-review。当前 user-owned private repository 的 GitHub Free
   计划不支持 required reviewers，因此 promotion 的手动 dispatch 是人工控制边界；不得把该限制写成
   已有人审。
2. `CYUNLAB_RELEASE_TOKEN` 必须作为 repository secret 供自动 Draft/smoke 使用，并在 Environment
   中配置同名受保护副本供最终 promotion 使用。把 `NARRANT_RELEASE_SIGNING_PUBLIC_KEY`、
   `NARRANT_UPDATER_PUBLIC_KEY`、`NARRANT_UPDATER_BRIDGE_TAG` 和 normal-only 的
   `NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY` 作为 repository variables 供自动 Draft/smoke 使用，
   同时在 Environment 中配置同名副本；Environment 副本是最终 promotion 的受保护授权，必须逐字匹配
   获批 tag 的预期值。bootstrap 期间不得设置 previous-release key。
3. OSS 首次 Stable 迁移期间，在 repository 与 Environment 同时设置
   `NARRANT_OSS_BOOTSTRAP_TAG=vMAJOR.MINOR.PATCH`。第一次成功后删除两处变量；之后只允许 normal
   promotion，不能用 GitHub Release 历史代替 OSS previous Stable 身份。
4. Environment 还必须配置 `OSS_BUCKET=xlcyun-public`、`OSS_REGION=cn-shenzhen`、
   `ALIBABA_CLOUD_OIDC_PROVIDER_ARN` 与只允许 Narrant 前缀写入的
   `ALIBABA_CLOUD_ROLE_ARN`。OIDC role 不使用长期 AccessKey；bucket 必须启用 Versioning，公开策略
   只读 `narrant/releases/*`、`narrant/candidates/*` 和 Stable manifest。
5. workflow 始终 checkout 默认分支上的可信脚本，再把 source tag/SHA 当作不可信数据验证，
   不执行 tag 中的脚本。

`update-smoke.yml` 需要读取 Draft，因此 runner 也使用 `CYUNLAB_RELEASE_TOKEN`。脚本只执行
Release GET/asset download；token 只进入 `Authorization` header，不进入 URL、argv、artifact、
evidence 或错误正文。最终写操作只存在于受保护的 promotion job。

个人账户下的私有仓库在 GitHub Free 计划中不提供 Artifact Attestations。当前 promotion 在下载
evidence 前调用 `scripts/verify-evidence-run.mjs`，通过只读 Actions API 固定验证 run 属于
`XLCYun/narrant`。正常自动路径要求 `.github/workflows/release.yml` 的 tag push run 绑定 exact
source tag/SHA，并且恰好有两个构建 artifact、两个 target artifact 与一个 aggregate artifact；手动
重试路径仍要求 `.github/workflows/update-smoke.yml` 的 `workflow_dispatch` run 绑定默认分支及 promotion
checkout 的可信 SHA，并且恰好有三个 smoke artifact。两种路径都要求 attempt 为 1、结论为 success，
且每个 artifact 的 ID、名称、大小、
GitHub SHA-256 digest、run/ref 与 archive URL 都必须匹配。下载后再要求 exact-four evidence 文件和
byte-exact `.sha256` companion。升级 GitHub 计划或调整仓库形态后，应恢复 Artifact Attestations，
不能在文档中把当前替代门禁描述成 attestation。

## 正常发布流程

1. 在私有仓库创建并推送经批准的 `vMAJOR.MINOR.PATCH` tag。确认 tag 解引用为默认分支上的
   exact 40 位 lowercase commit SHA。
2. 等待 `release.yml` 完成。它先固定当前匿名 published Stable 的 tag 与 manifest 原始字节 SHA-256，
   构建 macOS arm64 和 Windows x64，签名 updater packages，向 `cyunlab/narrant` 创建或完整替换一个
   Draft；随后自动以 `mode=normal` 运行两平台真实更新 smoke。Draft 必须恰好含八个资产：两个安装包、
   两个 updater packages、两个 `.sig`、`latest.json` 和 `SHA256SUMS`。任何 smoke 失败都会使 release
   run 失败并保留 Draft，不会自动发布 Stable。
3. 从 release job 日志取得固定 public Release database ID，或用 token 做只读查询：

   ```bash
   gh api repos/cyunlab/narrant/releases/tags/vMAJOR.MINOR.PATCH --jq .id
   ```

   同时记录 private source tag/full SHA 与 public Release ID/tag；public tag 不能代替 private SHA。
4. `release.yml` 已在 Draft mutation 前记录 previous Stable tag 和 manifest SHA-256，并把它们传给自动
   smoke。若自动 run 因 runner 临时故障需要重试，才手动运行 `Native application update smoke evidence`：

   - `mode=normal`；
   - 传入 source tag/full SHA、fixed public Release ID/public tag；
   - 传入 previous Stable tag 与刚记录的 manifest SHA-256。

   workflow 在现有自托管 macOS ARM64 与 Windows x64 桌面 runner 上回读、核验 Draft 的八个
   资产与上一 Stable 包，但不安装或启动正式应用。它从相应的精确源码构建独立的
   `Narrant Upgrade Test` 旧版/候选包，使用临时 updater 密钥；macOS 测试 app 使用 ad-hoc 签名。
   正式 app/DMG 的生产代码签名、公证及 updater 信任链单独静态验证。
5. 两平台实际测试包必须完成检查、下载、验签、持久化暂存、正常退出后安装、同路径替换、
   不自动重启、手动启动候选，以及模拟项目和主题/语言设置保留。TLS gate 使用测试进程独有的
   代理和 CA 文件，不操作 hosts 或系统信任。aggregate 只接受 schema v2、恰好两个 target 的
   `isolated-native` evidence 及逐字节 checksum；证据同时包含测试源码/叠加差异和正式资产身份。
   这证明独立身份下的升级行为，不证明正式包本体、日常安装目录或生产签名组合下的实际升级。

6. 自动路径记下成功的 **release workflow run ID**；手动重试路径才记下独立 smoke workflow run ID。
   手动运行 `Promote public Stable release`，输入完全相同的 source/public/previous identity 和对应
   evidence run ID。GitHub 计划支持 required reviewers 时等待独立
   Environment 审批；当前 GitHub Free 配置由 dispatch 操作者在运行前完成人工核对。
7. 操作者核对 source tag/SHA、public Draft ID/tag、Release Notes、previous Stable 和 smoke run。
   job 先验证可信 workflow run 与该路径的 exact artifact set，再验证 exact-four 下载文件，随后通过
   固定 public Release ID 下载和验签八个 Draft assets，重验 freshness/两目标/previous identity，并在
   mutation 紧前再次校验 Draft ID、tag、asset ID/size/state。它先把 content-addressed 安装包、updater、
   签名、checksums 与 candidate manifest 写入 OSS 并逐字节回读；随后只 PATCH 该 Release 为 Latest。
   回读 GitHub fixed ID 后，才在 append-only promotion lock 下原子替换 OSS Stable manifest，并写后回读。
8. 从一台不带 Actions 凭据的机器匿名读取 Stable endpoint，确认 version、两 target URL 和签名。
   不要在已发布 Release 上“修补”资产；任何内容变化都发布更高 SemVer。

## 分支试运行与历史 bootstrap

开发分支可手动运行同一 workflow 的 `mode=preflight`，传入 exact `source_sha` 和
`previous_stable_tag`。它使用当前分支源码及既有 runner，不访问生产私钥、不创建 Draft、tag 或发布。
源码版本与上一 Stable 相同时，仅测试包的候选版本增加一个 patch；记录标记为
`isolated-native-preflight`，正式包尚未核验，promotion 会拒绝这种结果。
`preflight_target` 可选择 Windows、macOS 或两者，当前默认 Windows。用户于 2026-10-16
暂停 macOS 升级验收，恢复前仅选择 Windows；普通 macOS 质量检查与 E2E 不在暂停范围内。
分支试运行将包、签名、构建记录和诊断保留到当前 runner 的
`RUNNER_TOOL_CACHE/narrant-update-preflight/<run-id>-<attempt>`，路径写入 job summary。
这个副本供失败复现，结束排查后可按精确目录清理；不含测试私钥，也不产生正式发布资格。
分支 Artifact 上传失败时保留警告和本地副本；正式发布仍要求上传及双平台聚合全部成功。
`preflight_baseline=previous-stable` 保持上一 Stable 源码；`candidate` 使用当前修复源码构建
两个不同测试版本，并将证据标为 `same-source-version-regression`。后者用于验证修复后的升级
链路，不能证明上一 Stable 可升级，也不能替代正式发布要求的 previous Stable 源码与双平台证据。
当前 v1.2.14 存在检查/下载任务编号错配，需要人工安装修复版过渡；原失败保留在验收记录中。
本机执行同一链路使用 `pnpm test:upgrade:build <baseline-sha> <baseline-version> <candidate-sha>
<candidate-version> <new-output-directory>`，随后 `pnpm test:upgrade:run <test-builds.json> <new-evidence-directory>`。
运行需要当前交互桌面；记录环境应如实填写已有 `RUNNER_NAME` 与 `RUNNER_ENVIRONMENT=self-hosted`。

2026-10-16 已匿名读取 OSS Stable endpoint，当前为 v1.2.14；当前升级驱动只执行 normal。
以下保留早期迁移流程作为历史记录，不是当前共享桌面可执行步骤：

### 历史一次性 OSS Stable bootstrap

v1.1.5 没有 updater，不能伪造 v1.1.5→候选的自动升级证据。首个 updater-capable 版本采用一次性
bootstrap：

1. 在 repository 与 `stable-promotion` Environment 同时设置 exact `NARRANT_OSS_BOOTSTRAP_TAG`。
2. 正常生成该 tag 的完整 Draft。
3. 运行 smoke 时选择 `mode=bootstrap`，previous Stable 两项必须留空。两平台证据只声明：签名安装包
   安装、exact installed candidate 启动、内嵌 endpoint/public key、候选版本、损坏 staging 被恢复清除、
   正常关闭后 clean relaunch；它不声明旧版本自动升级。
4. promotion 也选择 `mode=bootstrap`。protected variable 必须与 source tag 完全相同，previous-release
   key 必须缺失，而且 OSS Stable object 必须不存在。其他 OSS 读取错误不能冒充 object 不存在。
5. 第一次成功后删除 repository 与 Environment 两处 variable，并永久使用 `mode=normal`。保存 bootstrap
   workflow run、两份 evidence、checksums、Actions artifact identity 和 Release ID 作为一次性审计记录。
   若将来恢复 Artifact Attestations，也保存 attestation。不得因重跑方便而复用 bootstrap。

旧 GitHub updater bootstrap 已于 2026-09-03 完成：source `v1.2.13` /
`fc7ae6e060dc5520937edcd598971728f26f17d1`，release run `33648114309`，public Release ID
`381418321`，成功 smoke run `33658283469`（aggregate artifact `9857902214`），promotion run
`33659151005`。该记录不等于 OSS Stable 已 bootstrap；迁移时仍需按上节设置一次性 OSS tag。

## 失败与恢复

### Draft 或 smoke 失败

- 失败发生在 promotion PATCH 前时，公开 Stable 不变。保留 Draft，查明原因后可对同一个 Draft 重跑
  `release.yml` 的完整 reconciliation；脚本会拒绝任何已发布 Release。
- 部分 Draft、重复/额外资产、asset ID/size/bytes 漂移、错误 signature/manifest/checksum 均不得人工
  绕过。重新生成完整八资产，不要只补单个文件后继续旧 smoke evidence。
- smoke 失败后检查测试进程、临时 CA、测试安装和数据的清理结果。若清理失败，保留日志并只修复
  本次测试拥有的资源；不得删除日常 Narrant 数据，也不得把失败证据用于 promotion。
- evidence 缺失、重复、超过 24 小时、checksum 变化、Actions run/artifact identity 失败或
  source/public/previous identity 不匹配时重新运行完整两平台 smoke，不能手工编辑 evidence 或
  checksum。已启用 Artifact Attestations 时，attestation 失败同样必须完整重跑。

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

首次发布密钥的实际备份位于阿里云 OSS bucket `xlcyun-narrant-secrets`（`cn-shenzhen`）；bucket 已启用
versioning、服务端 AES-256 加密和 Block Public Access。公开密钥文件 SHA-256 为
`1e3198fb0b3070d315693e51ce65f3a3cb84bef434e0f52c3c66a467a4695acb`，加密私钥对象 SHA-256 为
`4ecdca63ef47a762404dabaa2ce2d79acac17ca7bdb743a9cd8dc20602b0c220`。解密密码不存入 OSS：当前恢复
材料保存在 macOS Keychain service `com.cyunlab.narrant.updater-signing-key.v1`、account `XLCYun`。
恢复演练时必须先核对上述 digest，再从 Keychain 取得密码；不得把密码复制进本仓库、GitHub Actions
artifact 或发布资产。
该 OSS 对象是异地加密云备份，不等同于离线介质；仍应另行建立并演练一份真正离线的加密副本。

- updater 私钥是已安装客户端的长期信任根。至少保留一份离线加密备份；私钥密文与解密材料分开保存，
  分别限制访问并记录责任人。定期在隔离环境恢复副本、签名 disposable package，再用对应 public key
  验证；不得把恢复材料上传到 Actions、Release、cache、artifact、日志或支持包。
- **私钥丢失**：旧客户端不会接受新密钥签名。停止发布，恢复经验证的离线备份。无法恢复时不能仅替换
  Actions secret；需要用户手动安装重新建立信任的新版本，并进行安全/产品评审。
- **疑似私钥泄露**：立刻停止 promotion、撤销相关 CI 访问并保存审计记录。攻击者若拥有旧私钥，也能签
  bridge，因此不能把普通自动轮换当作充分处置；先完成事件响应和独立安全评审，必要时要求手动安装。
- **计划轮换**：只能在旧私钥仍可信时执行以下 old-key-signed bridge；bootstrap 禁止 bridge。

  1. 离线生成并备份 successor keypair。选定一个尚未发布的 exact bridge tag。
  2. 保持 `TAURI_SIGNING_PRIVATE_KEY` 为 old private key；把 repository variables 和
     `stable-promotion` Environment variables 同时设置为：
     `NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY=old public key`、
     `NARRANT_RELEASE_SIGNING_PUBLIC_KEY=old public key`、
     `NARRANT_UPDATER_PUBLIC_KEY=successor public key`、
     `NARRANT_UPDATER_BRIDGE_TAG=exact bridge tag`。不得把公钥正文写进 artifact 或日志；流水线只把
     当前/候选两个 SHA-256 identity、previous-release key SHA-256 和 bridge 状态写入 receipt/evidence。
  3. 推送 exact tag。`release.yml` 自动创建 Draft，本版 package 必须由 old key 验签，而候选 binary
     必须内嵌 successor key。以 `mode=normal` 运行两平台 smoke：previous Stable 的配置与 candidate
     package signature 都证明 old key；更新后的 candidate 配置证明 successor key。
  4. 审批 promotion 前，核对 Environment 中 exact bridge tag 与三个 key identity。promotion 会重新下载
     Draft，并要求受保护值重新计算的摘要逐字匹配 evidence，之后才允许发布 B。
  5. B 成为 Stable 且采用情况确认后，为**更高版本 C**切换
     `TAURI_SIGNING_PRIVATE_KEY=successor private key`，把 `NARRANT_RELEASE_SIGNING_PUBLIC_KEY` 与
     `NARRANT_UPDATER_PUBLIC_KEY` 都切为 successor，并清空 repository/Environment 的 bridge tag；但
     `NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY` 必须继续保留 old key，因为 C admission 安装的 previous
     B archive 仍由 old key 签名。C smoke 必须证明：old 验 B archive、B 内嵌 successor 验 C package、
     C 内嵌 successor。
  6. 只有 C 已通过 protected promotion 并成为 Stable 后，才把 repository/Environment 的
     `NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY` 切为 successor，供 D 及后续 normal release 验证 previous
     archive。此后 previous/current/candidate 三个角色通常都为 successor；保留 old key 的离线退役记录。

## 每次发布检查表

- [ ] private tag/full SHA 属于默认分支；版本在 package、Tauri、Cargo 中一致。
- [ ] public Draft ID/tag 已单独记录，八资产集合、Release Notes 和签名完整。
- [ ] macOS Developer ID/codesign/Gatekeeper/notarization/stapling 全部通过。
- [ ] Windows 是 x64 current-user NSIS；首期不要求 Authenticode，但 updater minisign 必须通过。
- [ ] normal evidence 绑定真实 previous Stable manifest 与 previous-release signing key SHA-256；bootstrap
      不携带 previous key 且只在一次性首版使用。
- [ ] exactly 两份 fresh isolated-native evidence、checksums、可信 Actions run 与 exact-three artifact identity
      均通过；启用 Artifact Attestations 时 attestation 也通过。
- [ ] dispatch 操作者已核对全部身份；GitHub 计划支持时，`stable-promotion` required reviewer 也已独立批准。
- [ ] promotion 二次回读 fixed Draft 后才发布；published Release 未被自动化编辑。
- [ ] 匿名 Stable endpoint 已复核；发布记录与 evidence run ID 已归档。
- [ ] 管理员理解 immutable releases 未启用，并确认整个窗口没有并发 Draft/Release 修改。

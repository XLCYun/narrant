# Application Update 发布运维

源码、Release、应用更新和语言支持包统一在 `XLCYun/narrant`。生产 Stable 入口：

```text
https://github.com/XLCYun/narrant/releases/latest/download/latest.json
```

CI 使用 GitHub-hosted `macos-15`（ARM64）和 `windows-2022`（x64），正式包验收不在开发机运行。
保留应用身份 `com.cyunlab.narrant`，已有数据目录、凭据和 Apple Developer ID 身份不变。
架构依据见 [ADR-0036](adr/0036-use-one-public-github-repository-for-distribution.md)。

## GitHub 配置

发布构建需要以下 repository secrets：

- `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：updater minisign 私钥与密码。
- `APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、
  `APPLE_API_ISSUER`、`APPLE_API_KEY`、`APPLE_API_PRIVATE_KEY`：macOS 签名、公证。

需要以下 variables：

- `NARRANT_RELEASE_SIGNING_PUBLIC_KEY`：验证本版 updater 包的 outer-base64 minisign 公钥。
- `NARRANT_UPDATER_PUBLIC_KEY`：候选应用内嵌、用于验证下一版的公钥；普通发布与上一项相同。
- `NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY`：normal 验收中验证上一 Stable 包；bootstrap 不使用。
- `NARRANT_UPDATER_BRIDGE_TAG`：仅计划换钥时设置 exact tag，平时为空。
- `NARRANT_GITHUB_CLIENT_ID`、`NARRANT_GITHUB_APP_SLUG`：GitHub Identity 构建配置。

创建 `stable-promotion` Environment，限制默认分支，并配置 required reviewers、禁止自我审批。
将四项 updater 公钥/bridge variables 同步为 Environment 的受保护值；promotion 使用该授权二次复核。
首次发布临时在 repository 和 Environment 配置相同的 `NARRANT_FIRST_UPDATER_TAG`，成功后删除。
实际 reviewer 配置必须在仓库 Settings 核实，不能因 YAML 写了 Environment 就认定已有独立审批。

不再需要跨仓库 token、OSS/OIDC 角色或 self-hosted runner 标签。工作流使用本仓库 `GITHUB_TOKEN`，
只有 Draft、promotion 和语言包发布 job 有 Contents 写权限；smoke 和 Actions 证据查询只读。
令牌只进入请求头，不进入 URL、argv、artifact 或日志。

## 正常发布

1. 在默认分支上确认 package/Tauri/Cargo 的版本一致，推送获批的 `vMAJOR.MINOR.PATCH` tag。
   不移动或复用已发布 tag。`release.yml` 自动复用 quality-gate；不要单独手动触发 quality-gate。
2. release 工作流记录匿名 previous Stable 的 tag 与 manifest 原始字节 SHA-256，构建两平台正式包。
   同仓库 Draft 必须恰好有八资产：两个安装包、两个 updater 包、两个 `.sig`、`latest.json`、`SHA256SUMS`。
3. Draft 回读验证后自动调用 `update-smoke.yml`。两个平台均安装正式上一版本，以临时 TLS gate 提供
   候选 manifest 和包，完成真实升级、原路径替换、不自动重启、候选重开及项目/主题/语言保留检查。
   生产包没有测试身份或观察 IPC。模拟数据保留的检查读取候选退出后的实际持久化文件。
4. smoke 失败保留 Draft，不发布 Latest。仅 runner 临时故障时可从默认分支手动重试 update-smoke，
   传入相同 source tag/SHA、Release ID/tag、previous tag/manifest SHA-256；不要放宽验证门禁。
5. 记录成功的 release workflow run ID；若手动 smoke 重试则使用该独立 run ID。
   从默认分支运行 `Promote public Stable release`，填写相同的 source/Release/previous 身份和 evidence run ID。
6. 审批前核对 Notes、源码 SHA、固定 Release ID、资产签名和完整两平台证据。
   job 先校验可信 run/ref/commit 与精确 artifact 集合，再下载证据和 Draft 二次验证。
   mutation 前重新检查 Draft ID/tag/asset ID/size/state，随后只发布这个 Release 并标为 Latest。
7. 匿名回读 Stable 入口，确认版本、两平台 URL 和签名。已发布内容不允许修补，变更必须发更高版本。

证据验证是 Actions API + artifact digest + 内容校验，不是 Artifact Attestation。
normal 自动路径绑定本仓库 tag push 的 release run；手动重试绑定默认分支可信 SHA 的 update-smoke run。
历史独立测试包证据不能获得当前正式包 promotion 资格。

## 首个 GitHub Stable

本仓库此前没有 Release。旧渠道客户端必须手动安装一次新渠道修复版，不能用旧渠道发布记录冒充
本仓库 previous Stable。v1.2.14 还包含检查/下载任务编号缺陷，不能承诺原地自动升级。

1. 选择高于旧版的首个版本，在 repository 和 `stable-promotion` Environment 设置同一个 exact
   `NARRANT_FIRST_UPDATER_TAG`。bootstrap 不传 previous tag/hash/key。
2. release 工作流仅对此 tag 选择 bootstrap；分别验证 macOS 和 Windows 正式签名安装、版本、
   内嵌信任配置、损坏暂存拒绝与干净重启。报告明确标为首版安装验收，不称为旧版升级成功。
3. promotion 检查没有已发布 updater manifest；成功后立即删除两处 bootstrap 变量。
   后续发布执行完整 previous Stable → candidate normal 验收，不能重新启用 bootstrap 跳过失败。

## 失败恢复

- Draft/验收失败：修复原因后重建候选；任何资产变化都使旧证据失效，需重跑两平台验收。
- promotion 失败：先只读核实固定 Release ID 是否已经发布，以及匿名 Latest 是否相符；不要盲目重发、
  覆盖已发布资产或降级 Latest。若已发布后发现问题，发布更高版本修复。
- 候选运行失败：保留错误诊断，不宣称未完成的检查通过；临时 runner 的 hosts、CA、应用与夹具由 cleanup 回收。
- workflow token 自动过期；其他签名凭据泄露按各自凭据轮换。日志不得输出 secret 值。

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

## 发布检查表

- [ ] source tag/full SHA 属于默认分支，source tag 与 Release tag 相同，版本一致。
- [ ] Draft 的八个资产、Notes、manifest 和签名完整，固定 Release ID 已记录。
- [ ] macOS 签名、公证与 stapling 通过；Windows current-user NSIS 与 updater minisign 通过。
- [ ] 两平台 `real-native` evidence、checksums、可信 Actions run 与精确 artifact 身份全部通过。
- [ ] normal 绑定实际 previous Stable；bootstrap 只用于无已发布 updater 的首版，且明确覆盖范围。
- [ ] Environment 保护及审批实际生效，promotion 只修改经过二次验证的固定 Draft。
- [ ] 匿名下载已回读，正式发布记录和 evidence run ID 可追溯。

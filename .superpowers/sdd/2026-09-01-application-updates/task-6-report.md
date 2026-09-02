# Task 6 report — native update evidence and protected Stable promotion

## Outcome

Task 6 已实现并提交为 `ea62bf3`（`feat(release): gate stable promotion with native evidence`）。
实现只增加私有仓库中的 native smoke、evidence 与受保护 promotion 能力；未连接 GitHub、未读取或
修改真实 secret/variable/environment/Release，也没有发布任何版本。

## Implementation

- `.github/workflows/update-smoke.yml`
  - 只允许手动输入并 checkout 默认分支上的可信脚本；source tag 必须解引用为默认分支祖先上的 exact
    40 位 SHA。
  - matrix 恰好是 `darwin-aarch64`（`macos-15`/arm64）与 `windows-x86_64`
    （`windows-2025`/x64）。每个 runner 都用 `CYUNLAB_RELEASE_TOKEN` 对固定
    `cyunlab/narrant` Draft Release ID 重新下载八个资产并做完整 bytes/size/SHA、manifest、
    SHA256SUMS 和 minisign 验证。
  - normal 模式匿名回读并验证 previous published Stable；bootstrap 模式明确不声明 previous upgrade。
    两个平台分别上传 JSON evidence 与 byte-exact checksum，aggregate 再次下载 Draft、要求 exact
    two-target set，并对四个固定 evidence 文件生成 GitHub provenance attestation。
- `.github/workflows/promote-stable.yml`
  - 只由 `workflow_dispatch` 启动，job 固定进入 `stable-promotion` Environment；没有
    `release.published` 依赖，也没有 `contents: write`。
  - 按指定 private run ID 下载 aggregate artifact，并对四个 exact 文件逐个执行 `gh attestation
    verify`，约束 private repository、`update-smoke.yml` signer、默认分支 source ref，并拒绝
    self-hosted runner provenance。
  - Environment 审批后重新按固定 public Release ID 下载/验签 Draft，重验 freshness、source、
    previous/bootstrap、两目标与全部 digest。mutation 紧前再次按 ID 回读 Draft 和八资产 identity，
    之后只把该 Draft PATCH 为 published/latest 并回读 Latest。
  - bootstrap 默认 fail closed；只有 protected `NARRANT_FIRST_UPDATER_TAG` 与 candidate 完全相同，
    且当前 published Latest 不含 `latest.json` 时才开放。
- `scripts/public-draft-assets.mjs`
  - token 只进入 GitHub API `Authorization` header；asset API redirect 手工跟随，第二跳移除 token，
    且只接受默认 TLS 端口、无凭据/fragment 的 GitHub 官方 release asset 域名。
  - 下载使用流式 SHA/size 校验，receipt 绑定 public repository、Release ID/tag、八个 asset ID/size/
    SHA-256、manifest SHA 与完整已验证 manifest。
- native driver 与平台 adapter
  - normal 流程真实安装 previous updater-capable Stable，启动 exact installed app，经临时 TLS gate
    观察 check/download/durable staging，原生关闭主窗口以复用 production normal-exit
    `ApplicationShutdown::stop_once`，确认同路径安装、五秒无自动 relaunch，并由 harness 手工启动候选。
  - macOS 验证 thin arm64 app、bundle/version、codesign、Gatekeeper、stapling，并用独立 Swift
    Accessibility helper 对唯一主窗口执行 AXPress；Windows 只接受唯一 HKCU current-user NSIS 安装，
    使用 `CloseMainWindow`，明确不接受 MSI。
  - bootstrap 验证 signed candidate install/launch、内嵌 endpoint/public key，并注入损坏的 production
    schema-2 staging generation，观察启动恢复清除后正常关闭与 clean relaunch；不伪造 previous upgrade。
- TLS gate 与 evidence
  - gate 只服务 shipped GitHub latest path 和 candidate 当前 tag/target package path，bytes 均来自已验证
    Draft。临时 CA/leaf、port-443 helper、hosts 原字节备份/恢复、DNS flush 和 trust cleanup 都封装在
    runner-only adapter 中；cleanup 错误不覆盖 native 主错误。
  - evidence 绑定 `XLCYun/narrant` source tag/full SHA、public Release ID/tag、八资产 receipt、candidate
    manifest SHA、previous manifest/package/signature SHA、runner OS/arch/image、mode、freshness 与固定真实
    checkpoints；目录必须恰好包含两个 JSON 和两个 checksum companion。
- `docs/application-update-operations.md`
  - 记录 secrets/variables、protected Environment、normal 发布、一次性 bootstrap、Draft/smoke/promotion
    失败恢复、token 泄露、私钥离线备份/丢失/泄露、old-key-signed bridge rotation 和每次发布检查表。
  - 明确 immutable releases 不启用、管理员必须避免 guard→mutation 微窗口内并发修改，而且自动化绝不
    编辑 published assets。

## Verification

最终未提交树和 implementation staged tree 均完成了对应 whitespace 检查；提交前执行结果：

- `pnpm test:scripts`：`112 passed; 0 failed`。
- `pnpm lint`：通过。
- `node --check`：10 个新增 production `.mjs` 文件全部通过。
- Ruby Psych `YAML.load_file`：`update-smoke.yml` 与 `promote-stable.yml` 均通过解析。
- `git diff --check` 与 `git diff --cached --check`：通过。
- 敏感边界静态扫描：未发现 DSH/四目标/OSS endpoint 假设；workflow tests 证明 exactly two targets、
  pinned Actions、least permissions、Environment、cross-run artifact、attestation、bootstrap fail-closed，
  以及 token 不进入 argv/URL。

本机没有安装 `actionlint`，因此没有联网安装；workflow 验证由 YAML parser 与 3 条 repository-owned
static tests 覆盖。本机 Node `v26.5.0` 超出仓库声明的 `>=22.13 <26`，pnpm 输出 engine warning；测试
与 lint 均成功，两个 workflow 固定使用受支持的 Node 24。Ruby 另报告 `/opt/homebrew/bin` 为
world-writable PATH warning，但 YAML 解析成功。

没有修改 Rust，因此未运行 Cargo。没有手动触发 quality-gate。

## Hosted/native residuals

1. 本地验证没有、也不能声称执行过 `macos-15`/Windows hosted runner 上的真实 signed previous Stable
   安装、Accessibility/CloseMainWindow、系统 trust store、port 443、hosts/DNS、normal-exit 安装或候选
   手工 relaunch。第一次真实运行仍需确认 macOS runner 已预授权 Accessibility helper，以及两个平台的
   安装器布局与 runner 权限符合 runbook。
2. 没有实际调用 GitHub Draft asset API、cross-run artifact download、artifact attestation verify、
   protected Environment reviewer 或 Release PATCH/readback；这些 hosted 行为由 workflow 门禁和本地
   injectable fixtures/static assertions 覆盖，必须由首次受控 CI run 证明。
3. GitHub mutable Release 没有 guard→PATCH 的跨调用 CAS，而且产品明确不启用 immutable releases。
   当前实现使用 fixed-ID byte re-fetch、mutation 紧前 identity guard 与 mutation 后 Latest readback；
   管理员仍必须在 smoke/promotion 窗口避免并发修改。
4. 自动化从不编辑 published assets，但若网络在 PATCH 成功后中断，维护者必须先按固定 ID 和 Latest
   做只读恢复判断，不能盲目重跑；runbook 已记录这一操作边界。

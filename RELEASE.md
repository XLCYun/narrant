# 发布 Narrant

源码、应用 Release、应用更新和语言支持包统一位于 `XLCYun/narrant`。
应用发布从稳定版本 tag 开始，经双平台构建、Draft 验证及正式安装升级验收后，
由维护者手动启动受保护的 promotion 流程。正式步骤、凭据、bootstrap 与失败恢复以
[Application Update 发布运维](docs/application-update-operations.md)为准。

## 准备版本

先同步默认分支和标签，确保工作区干净。`release:prepare` 接受不带 `v` 的完整稳定 SemVer，
目标版本必须高于当前版本与已有稳定标签。以下版本仅作示例，执行时选择实际待发布版本：

```bash
git fetch origin --tags
pnpm release:prepare 1.2.15
```

该入口位于 `scripts/release/release-prepare.mjs`，会：

1. 检查 package、Tauri、Cargo manifest 与 Cargo lock 的当前版本一致。
2. 同步四处版本，创建 `chore(release): v<version>` 提交和 annotated tag。
3. 失败时回滚本次版本准备；成功后保留本地提交和 tag，不执行推送。

检查提交和 tag 后再显式推送；发布 source 必须属于默认分支。已发布 tag 不移动或复用，
失败恢复和正式发布后的修复遵循运维说明。

## 构建、验收与公开

`release.yml` 复用完整质量门禁，在 GitHub-hosted macOS ARM64 和 Windows x64 上构建正式包。
macOS 继续执行 Developer ID 签名、公证及 stapling；Windows 当前可没有 Authenticode，
两个平台的 updater minisign 签名都必须保留。

Draft 包含两个安装包、两个 updater 包、两个签名、`latest.json` 和 `SHA256SUMS` 共八资产。
固定 Release ID、source SHA、资产字节与签名经回读校验后，自动进入双平台 `update-smoke`。
normal 验证上一 Stable 到候选的真实升级，首版 bootstrap 按明确授权单独报告安装验收。

维护者核对候选及证据后，手动启动 `Promote public Stable release`。该流程在发布前再次
检查可信 Actions run、双平台证据、Draft 和 previous Stable 身份，只发布固定候选。
不要绕过该门禁在 Release 页面直接公开 Draft。发布后匿名回读确认；已发布资产不原地修改。

## 配置与开发入口

本仓库使用 `GITHUB_TOKEN`，不需要旧分发仓库 token。签名 secrets、updater 公钥角色、
受保护 Environment、首版 bootstrap 与换钥安排见[发布配置](docs/application-update-operations.md#github-配置)。
密钥及密码不得进入源码、日志、artifact 或 Release。

日常聚焦验证与 PR 自动 `Complete CI` 规则见[开发说明](docs/development.md#合并前-ci)；
不额外手动触发 quality-gate。正式安装升级只在托管临时机器运行。
脚本目录及常用命令见[scripts 导航](scripts/README.md)，语言支持包独立 prerelease 见
[语言支持包发布](docs/language-support-packs.md#首次上线与后续发布)。

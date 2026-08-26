# 发布 Narrant

正式版本由私有源仓库中的稳定版本标签触发。工作流完成质量门禁，构建 macOS
和 Windows 安装包，并在 `cyunlab/narrant` 创建一个待人工检查的 Draft Release。

## 首次配置

在私有源仓库的 Settings → Secrets and variables → Actions 中配置以下 Repository
Variables：

- `NARRANT_GITHUB_CLIENT_ID`：GitHub App 的公开 Client ID。
- `NARRANT_GITHUB_APP_SLUG`：GitHub App URL 中的公开 slug。

配置以下 Repository Secrets：

- `CYUNLAB_RELEASE_TOKEN`：Fine-grained personal access token，只授权
  `cyunlab/narrant`，Repository permissions 中仅需 `Contents: Read and write`。
- `APPLE_CERTIFICATE`：Developer ID Application `.p12` 文件的单行 Base64。
- `APPLE_CERTIFICATE_PASSWORD`：导出 `.p12` 时设置的密码。
- `APPLE_SIGNING_IDENTITY`：`security find-identity -v -p codesigning` 输出中的
  Developer ID Application 身份全名。
- `APPLE_API_ISSUER`：App Store Connect API Issuer ID。
- `APPLE_API_KEY`：App Store Connect API Key ID。
- `APPLE_API_PRIVATE_KEY`：对应 `.p8` 私钥文件的单行 Base64。

在 macOS 上可用以下命令生成两个 Base64 值：

```bash
openssl base64 -A -in developer-id-application.p12
openssl base64 -A -in AuthKey_KEY_ID.p8
```

不要向 Actions 提供 GitHub App Client Secret、GitHub App 私钥或 Apple 凭据原文件。

## 准备版本

先同步远端分支和标签，并确保工作区没有未提交修改。传入不带 `v` 的完整稳定
SemVer：

```bash
git fetch origin --tags
pnpm release:prepare 1.2.0
```

命令会完成以下操作：

1. 检查 `package.json`、Tauri 配置、`Cargo.toml` 和 `Cargo.lock` 中的当前版本一致。
2. 将四处版本更新为指定版本。
3. 创建 `chore(release): v1.2.0` 提交。
4. 创建 annotated tag `v1.2.0`。

命令不会推送。检查提交和标签后，显式推送：

```bash
git show --stat v1.2.0
git push origin HEAD v1.2.0
```

标签可以来自任意分支；创建合法稳定版本标签的人等同于拥有发布权限。因为标签
触发的工作流来自该标签对应的提交，能够创建标签的人也可能修改工作流并接触发布
Secrets。仓库应只向可信发布者授予写权限。标签一旦推送便不得移动或重建，发现
问题时应准备新的补丁版本。

## 自动构建

`.github/workflows/release.yml` 只接受 `vMAJOR.MINOR.PATCH` 标签，并要求标签版本
高于已有稳定标签且与四处版本声明完全一致。它依次执行：

1. 可复用的完整质量门禁，包括 Rust、TypeScript，以及 macOS 和 Windows 的真实
   Tauri E2E。
2. 在 Apple Silicon runner 上构建、签名、公证并验证 DMG。
3. 构建未进行 Authenticode 签名的 Windows x64 NSIS 安装包。
4. 汇总上一个稳定版本以来的 commit 标题，生成公开 Release Notes。
5. 生成 `SHA256SUMS`，在 `cyunlab/narrant` 创建或更新 Draft Release 并上传资产。

同一标签重跑时，工作流只会更新仍处于 Draft 状态的 Release，并先删除旧资产。
它不会修改已公开的 Release。

## 人工公开

工作流成功后，在 `cyunlab/narrant` 的 Releases 页面检查：

- Release Notes 中没有不应公开的提交标题。
- 存在 Apple Silicon DMG、Windows x64 NSIS EXE 和 `SHA256SUMS`。
- Windows 未签名提示仍然准确。

检查完成后，在 GitHub 页面点击 Publish release。Actions 不会自动执行公开操作。

## 本地质量门禁

需要在发布前本地复核时，Rust 命令统一通过仓库级 Cargo 锁运行：

```bash
scripts/cargo-agent.sh fmt --all --manifest-path src-tauri/Cargo.toml -- --check
scripts/cargo-agent.sh clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --all-targets
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --doc
pnpm lint
pnpm build
pnpm test
```

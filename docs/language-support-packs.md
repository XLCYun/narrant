# 语言支持包构建与发布

应用首次进入需要分析的“结构改动”时按需下载，不会在打开 Review 或读取状态时访问分发源。
语言分析器收到完整安装目录后才启动；准备期间不占用 Review Workspace。

## 版本和内容

`resources/language-support-packs.json` 同时供 Rust 应用和 Node 打包脚本读取。改变任何文件内容时
提升对应包版本并保留历史产物，不覆盖同版本。首版两个包均为 `1.0.0`：

| 包 | 内容 | 平台 |
| --- | --- | --- |
| Rust | rust-analyzer 2026-09-07，MIT/Apache 许可证 | macOS arm64、Windows x64 |
| TypeScript / JavaScript | Node 24.18.0、TypeScript 6.0.3、TLS 6.0.0、@types/node 22.20.1、undici-types 6.21.0，随附上游许可证 | 同上 |

Rust 项目仍需要用户已有的 Cargo/rustc；标准库或完整依赖范围还需要对应工具链的 rust-src。
应用不运行 rustup 安装命令。纯仓库范围不加载标准库。支持包不包含 npm/pnpm，TS/JS 不会安装被审查项目的依赖；Rust 的“所有依赖”范围保留原有 Cargo 策略。

## 本地构建与验收

使用仓库支持的 Node 和 `pnpm install --frozen-lockfile` 后运行：

```sh
pnpm build:language-packs --all
# 仅当前目标；--native 会拒绝宿主架构不匹配，执行包内工具和 TS 引擎。
pnpm build:language-packs --target aarch64-apple-darwin --native
# 或 --target x86_64-pc-windows-msvc --native
```

默认输出 `dist/language-support-packs/<id>/<version>/<target>/{manifest.json,pack.tar.gz}`。
若后续要运行前端生产构建，请用 `--output .tools/language-support-pack-artifacts`，避免 Vite 清理 dist。
上游下载每次校验固定摘要，缓存位于 `.tools/language-pack-downloads`；不执行 npm 安装脚本。
归档按稳定文件顺序、固定时间及权限创建，不包含系统扩展属性。清单包含归档及每个文件的 SHA-256。

真实安装和 TS/JS 分析验证（先构建当前平台产物）：

```sh
NARRANT_LANGUAGE_PACK_FIXTURE="$PWD/.tools/language-support-pack-artifacts" \
  scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --lib \
  real_installed_typescript_pack_analyzes_without_development_tools -- --ignored
```

PR 上 `Language support packs` workflow 在两个原生 runner 构建并启动工具，保存完整产物。
它与应用发布、quality-gate 分开；不要手动触发 quality-gate。

## 首次上线与后续发布

生产地址固定为：
`https://updates.cyunlab.com/narrant/language-support-packs/<id>/<version>/<target>/`。
首次发布前由维护者配置受保护 GitHub Environment `language-support-packs`：

1. 配置 `OSS_BUCKET`、`OSS_REGION`、`ALIBABA_CLOUD_OIDC_PROVIDER_ARN`、`ALIBABA_CLOUD_ROLE_ARN`。
2. OIDC 信任限定本仓库的该 Environment 和 main 发布流程。短期角色只需要新前缀的读/写权限；
   不要扩大现有应用 Stable promotion 角色的权限。通过 bucket/CDN 为该前缀提供公开只读 HTTPS。
3. 合入 main 后运行 `Language support packs`，勾选 `publish`。该流程先原生构建验证，再进入
   Environment 保护，串行发布；从其他分支触发只允许构建，发布 job 不会运行。
4. 脚本先校验四份本地产物，再上传归档并回读，最后上传清单并回读；已有字节完全一致则复用，
   不同则失败。最后通过用户实际访问的 HTTPS 地址逐字节验证。此步骤成功后才能发布引用这些版本的应用。

发布信任来自受保护的仓库版本、OIDC 权限和固定 HTTPS 来源；SHA-256 验证下载损坏及清单/归档一致性，
不是独立的签名认证。当前没有额外的支持包签名基础设施。

不要删除仍被已发布应用引用的包。升级失败时可回退应用，其声明的旧版本仍可复用；支持包不追踪 latest。
本分支不会自动部署 OSS、创建云权限或发布应用；首次生产下载可用性必须以发布 job 的公开回读验证为准。

## 本地存储与故障恢复

当前用户应用数据目录的 `language-support-packs` 下：

- `installed/<id>/<version>/<target>`：完整不可变内容，含 `installed.json` 提交标记。
- `staging/<id>/<version>/<target>`：TempDir 承载本次下载和解压；普通失败/退出清理，崩溃后下次启动回收。
- `locks/`：同版本跨进程锁；使用中的版本不能删除，安装提交和删除互斥。

半包重试重新下载，完整包保留。设置页可删除未使用版本，包括旧版本；没有自动删除历史安装。
磁盘占用显示包内文件的逻辑字节总和，不包含临时下载或文件系统块开销。

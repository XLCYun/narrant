# 语言支持包构建与发布

应用首次进入需要分析的“结构改动”时按需下载，不会在打开 Review 或读取状态时访问分发源。
语言分析器收到完整安装目录后才启动；准备期间不占用 Review Workspace。

## 版本和内容

`resources/language-support-packs.json` 同时供 Rust 应用和 Node 打包脚本读取。改变任何文件内容时
提升对应包版本并保留历史产物，不覆盖同版本。历史首版为 `1.0.0`；当前 Rust、TS/JS 和 Python
使用未发布的 `1.1.0` 原生候选，最终双平台 CI 尚未执行：

| 包 | 内容 | 平台 |
| --- | --- | --- |
| Python | narrant-python，内嵌同源 ty 0.0.84 / Ruff、固定 typeshed、控制身份及实际目标锁定许可闭包 | macOS arm64 本地验收；Windows x64 待最终 CI |
| Rust | narrant-rust，固定 rust-analyzer 库 revision 9074e9b；包内包含实际目标的锁定依赖许可闭包 | macOS arm64 本地验收；Windows x64 待最终 CI |
| TypeScript / JavaScript | Node 24.18.0、TypeScript 6.0.3、TLS 6.0.0、@types/node 22.20.1、undici-types 6.21.0，随附上游许可证 | 同上 |

Rust 项目仍需要用户已有的 Cargo/rustc；标准库或完整依赖范围还需要对应工具链的 rust-src。
应用不运行 rustup 安装命令。纯仓库范围不加载标准库。支持包不包含 npm/pnpm，TS/JS 不会安装被审查项目的依赖；Rust 的“所有依赖”范围保留原有 Cargo 策略。

工具链检查在实际发现的 Rust 项目目录执行，包含 `src-tauri/` 这类直接子项目。
一个分析会话只配置一套标准库；同一仓库内多个独立项目若使用不同 sysroot，
标准库/所有依赖范围明确报错并提示切换为“仅仓库”，不把仓库根目录的默认工具链套给所有项目。

## 本地构建与验收

使用仓库支持的 Node 和 `pnpm install --frozen-lockfile` 后运行：

```sh
pnpm build:language-packs --all
# 仅当前目标；--native 会拒绝宿主架构不匹配，执行包内原生 helper 的真实分析与归档提取 smoke。
pnpm build:language-packs --target aarch64-apple-darwin --native
# 或 --target x86_64-pc-windows-msvc --native
```

默认输出 `dist/language-support-packs/<id>/<version>/<target>/{manifest.json,pack.tar.gz}`。
若后续要运行前端生产构建，请用 `--output .tools/language-support-pack-artifacts`，避免 Vite 清理 dist。
上游下载每次校验固定摘要，缓存位于 `.tools/language-pack-downloads`；不执行 npm 安装脚本。
归档按稳定文件顺序、固定时间及权限创建，不包含系统扩展属性。清单包含归档及每个文件的 SHA-256。

Rust 候选通过正常入口构建优化 helper，构建工具链固定 1.98.0；运行时保留项目已有 Cargo/rustc。
支持包包含控制身份、可执行入口、实际目标依赖许可和原始许可出处，不携带项目工具链。

```sh
pnpm build:language-packs --id rust --native --output .tools/language-support-pack-artifacts
node scripts/verify-native-analyzers.mjs .tools/language-support-pack-artifacts --language rust
```

该验收覆盖真实图/diff、三档配置、隔离安装、生产 Review DTO 及取消/错误；
历史独立 helper 证据与本地生产退役结果见 [迁移验证](research/existing-native-helper-migration-validation.md)。

Python 不需要用户 Python、venv 或 Node；同源 Ruff parser/AST/codegen、ty 数据库与内嵌 typeshed
编译进独立 `narrant-python`，应用通过公共 Native Host 构图。
输入支持 UTF-8 `.py/.pyi`、Python 3.7–3.14，只分析本侧仓库和固定标准库。详见
[Python 规格](specs/0023-python-language-analyzer.md)及[实现验收](research/python-implementation-validation.md)。

```sh
pnpm build:language-packs --id python --native --output .tools/language-support-pack-artifacts
node scripts/verify-python-analyzer.mjs .tools/language-support-pack-artifacts
```

验证脚本委托共享原生验收入口，从正常归档提取 helper，通过仓库 Cargo 包装器依次验证
固定历史图、各 minor 的语法/真实语义、真实安装租约、生产 Review DTO、错误与取消；
正常退出后清理临时工具目录。Python 同样使用 1.98.0 优化构建和共享 Cargo 许可收集器。
具体许可证据边界与本地结果见 [迁移验证](research/existing-native-helper-migration-validation.md)。

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
4. 脚本先校验三种语言的六份本地产物，再上传归档并回读，最后上传清单并回读；已有字节完全一致则复用，
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

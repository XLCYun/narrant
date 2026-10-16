# 语言支持包构建与发布

应用首次进入需要分析的“结构改动”时按需下载，不会在打开 Review 或读取状态时访问分发源。
语言分析器收到完整安装目录后才启动；准备期间不占用 Review Workspace。

## 版本和内容

`resources/language-support-packs.json` 同时供 Rust 应用和 Node 打包脚本读取。改变任何文件内容时
提升对应包版本并保留历史产物，不覆盖同版本。历史首版为 `1.0.0`；当前 Rust、TS/JS 和 Python
使用未发布的 `1.1.0` 原生候选，此前双平台验收通过；迁移 GitHub-hosted 后需对最终提交重新验收：

| 包 | 内容 | 平台 |
| --- | --- | --- |
| Python | narrant-python，内嵌同源 ty 0.0.84 / Ruff、固定 typeshed、控制身份及实际目标锁定许可闭包 | macOS arm64 / Windows x64 |
| Rust | narrant-rust，固定 rust-analyzer 库 revision 9074e9b；包内包含实际目标的锁定依赖许可闭包 | macOS arm64 / Windows x64 |
| TypeScript / JavaScript | Node 24.18.0、TypeScript 6.0.3、@types/node 22.20.1、undici-types 6.21.0，随附上游许可证 | 同上 |

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

每种语言、每个版本使用一个 `XLCYun/narrant` prerelease：

```text
https://github.com/XLCYun/narrant/releases/download/language-pack-<id>-v<version>/<target>-manifest.json
https://github.com/XLCYun/narrant/releases/download/language-pack-<id>-v<version>/<target>-pack.tar.gz
```

1. 配置受保护 Environment `language-support-packs`，限制 main 和维护者审批。
2. 本地及双平台验收通过后启用目录中对应版本（`published`），合入 main；运行 `Language support packs`，
   选择完整双平台、全部语言并勾选 `publish`。部分语言/平台验收不能发布。
3. 脚本先核验全部本地产物，再为每个语言版本创建 Draft prerelease，上传并逐字节回读两个平台的四个资产。
   全部成功后公开，保持 `prerelease: true`、`make_latest: false`，不会覆盖应用 Stable 的 Latest。
4. 发布后匿名回读客户端 URL；同版本只允许复用相同字节，已发布资产不修改。公开回读成功后才发布引用它的应用。

发布使用本仓库 `GITHUB_TOKEN`，权限限定受保护发布 job 的 Contents 写入。
下载只允许固定 GitHub 仓库与官方资产 CDN，保留清单/归档 SHA-256 校验；这不是独立的包签名认证。
当前目录的七种包均未启用发布。不要删除仍被已发布应用引用的版本，支持包不追踪 Latest。

## 本地存储与故障恢复

当前用户应用数据目录的 `language-support-packs` 下：

- `installed/<id>/<version>/<target>`：完整不可变内容，含 `installed.json` 提交标记。
- `staging/<id>/<version>/<target>`：TempDir 承载本次下载和解压；普通失败/退出清理，崩溃后下次启动回收。
- `locks/`：同版本跨进程锁；使用中的版本不能删除，安装提交和删除互斥。

半包重试重新下载，完整包保留。设置页可删除未使用版本，包括旧版本；没有自动删除历史安装。
磁盘占用显示包内文件的逻辑字节总和，不包含临时下载或文件系统块开销。

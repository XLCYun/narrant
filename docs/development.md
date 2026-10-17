# 开发与语言分析

## TypeScript / JavaScript 结构分析

在 Narrant 仓库根目录安装应用依赖：

```sh
pnpm install --frozen-lockfile
```

应用自身支持 Node `>=22.13 <26`，构建用 TypeScript 仍由 `package.json` 和
`pnpm-lock.yaml` 管理。TS/JS 结构分析使用独立支持包中的固定 Node 24.18.0、
TypeScript 6.0.3、@types/node 22.20.1 和 undici-types 6.21.0；安装与使用租约
由公共语言支持包服务管理。生产入口通过 Native Host 直接调用 Compiler/Language Service API。

开发验证在 macOS ARM64 或 Windows x64 使用对应目标构建本地候选：

```sh
node scripts/build-language-support-packs.mjs --id typescript-javascript --target aarch64-apple-darwin --output native-language-support-packs
node scripts/verify-native-analyzers.mjs native-language-support-packs --language typescript-javascript
```

Windows 目标为 `x86_64-pc-windows-msvc`。构建验证固定上游摘要，不运行包安装脚本；
验收包含真实图、归档安装与 Review DTO。当前 1.1.0 候选尚未发布；验证范围见原生迁移证据。

Rust 使用包含 `narrant-rust` 的 1.1.0 原生支持包候选，通过同一数据库提取语法、语义与原文。
构建 helper 固定 Rust 1.98.0，分析项目保留用户实际 Cargo/rustc、PATH 和 rustup 覆盖；
标准库及所有依赖范围需要该项目工具链已有 rust-src。所有依赖范围沿用用户网络政策，
仅仓库及标准库范围始终离线。分析不安装工具链、不运行项目构建脚本或过程宏。
加载、解析或必需工具失败都会使整次结构比较失败。此候选尚未发布。

Python 使用 1.1.0 的 `narrant-python` 原生候选和内嵌固定 typeshed，AST、原文和导航
来自同一不可变数据库，保持 Python 3.7–3.14 与仓库加标准库范围。
Rust、TS/JS、Python、Java、Go、C#、C/C++ 七套 helper 均经过同一 Native Host 与
`child_process::ProcessTree`；旧 LSP、独立 parser/server 协调和开发 setup 已退役。
正常构建按实际依赖收集许可，Python seahash 上游通知的未取回限制见
[来源记录](../native-helpers/python/licenses/sources.json)。七套语言的 Windows/macOS 验证已完成；迁移到 GitHub-hosted 后需重新验证 CI，支持包尚未发布。

结构视图共同分析 `.ts`、`.tsx`、`.js`、`.jsx`、`.mts`、`.cts`、`.mjs`、
`.cjs` 及 `.d.ts`、`.d.mts`、`.d.cts` 声明文件。固定 TypeScript 原生 AST 提取语法结构和对象自身
内容，Language Service 提供定义导航与语义证据。原生 JSDoc 推断会保留，
不使用另外的 JSDoc 解析器。直接绑定到 `const` 名称的箭头函数会成为 Callable；
内联回调保留在所属对象内容中，不产生匿名回调节点。自定义 JSX 组件会产生组件引用，
`div`、`button` 等内置标签不产生节点或组件关系；其属性中的表达式和回调仍参与
普通内容与关系分析。

默认语义上下文由仓库第一方源码、项目配置启用的标准库，以及固定 Node 声明闭包
组成，不包含业务第三方源码或类型。缺少业务依赖时，可靠关系会保留，无法确认的目标
保持未解析，并按前后两侧分别显示上下文说明。这套代码图是保守的静态语义结果，
不保证完整的 TypeScript 静态可靠性；输入路径检查和语言服务的文件读取控制也不构成操作系统级
沙箱。

同一比较侧的 Rust、TypeScript 和 JavaScript 共用 Review 已签出的 Git workspace。
分析器不再复制仓库源码、配置或标准声明；配置调整与 workspace 包解析映射只存在于内存，
标准声明从固定工具目录直接读取。临时目录仅保存 Narrant 自身的助手和控制消息，
退出时清理这些文件及语言服务进程，Git workspace 继续由 Review 生命周期管理。

生产夹具已覆盖现代 NodeNext、Bundler、JS/JSX、Promise、Node ESM/CJS 包装和
原生 JSDoc。TS 5 风格兼容夹具使用 CommonJS、旧式 Node moduleResolution、
baseUrl 和 paths，在固定 TS 6 引擎下保留可靠的本地调用，将冲突导入保持为未解析，
并显示原生弃用说明。这些代表性结果不表示使用项目安装的 TypeScript，也不构成对所有
TS 5/6 配置的兼容承诺。

原生 TS/JS 支持包、公共图与差异、隔离安装及 Review 已完成本地 macOS ARM64 验证；
Windows/macOS 验证记录见下方迁移证据；CI 环境变更后重新验收。受控配置与 workspace 支持边界见
[原生 Helper 说明](../native-helpers/typescript/README.md)，迁移断言与验证记录见
[原生迁移证据](research/0025-typescript-native-helper-migration.md)。

## 构建 Windows 安装包

安装项目依赖后，在 Windows 系统的项目根目录运行：

```powershell
pnpm build:windows
```

该命令仅构建 NSIS `.exe` 安装包。构建产物位于：

```text
src-tauri\target\release\bundle\nsis\Narrant_<version>_x64-setup.exe
```

## 本地开发

### 合并前 CI

指向 `main` 的 PR（包括草稿）在创建、推送新提交或重新打开时自动运行 `CI`。
入口没有文件路径过滤，同一 PR 的新运行会取消尚未结束的旧运行。

- `quality-gate.yml` 运行 macOS ARM64 / Windows x64 的 Rust 静态检查、单元、集成和
  文档测试，以及双平台真实 Tauri E2E；前端 Lint、类型检查、构建、组件与脚本测试也在此完成。
- `validate-language-support-packs.yml` 运行双平台支持包构建与全部七套 Native Helper 验收。
  PR 运行不上传大体积支持包；需要下载候选时使用原有手动语言包入口，上传产物保留 7 天。
- `Complete CI` 只在上述两套验收都成功时通过；失败、取消、跳过或缺失结果都不能放行。
  `main` 的必过检查使用此名称，合入前还要求分支包含最新 `main`。

PR 验收使用标准 GitHub-hosted 临时机器，不需要签名或发布凭据。签名、公证、Release 和
正式安装升级验收仍由发布流程执行；PR 通过不等于正式包升级已通过。真实模型 endpoint
等人工专项仍按各自说明执行，不纳入公开 PR 的确定性测试。

日常修改继续先做本地聚焦验证，无需为每次小改重复完整测试。修改已推送后，以当前 PR
完整 CI 的结果为合并依据；此前提交或部分专项的绿灯不能替代它。

Custom Babysit 的确定性 smoke、终端开发日志和数据保密边界见
[开发日志](agents/dev-logging.md) 与 [Babysit smoke test](agents/babysit-smoke.md)。

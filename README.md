# narrant

## TypeScript / JavaScript 结构分析

使用应用支持的 Node 版本安装项目依赖后，在应用仓库根目录显式准备分析工具：

```sh
pnpm install --frozen-lockfile
pnpm setup:typescript-analyzer
```

应用自身支持 Node `>=22.13 <26`，语言服务最低要求 `22.22.2`，因此准备分析工具时
应使用 `>=22.22.2 <26`。准备命令需要网络和 `tar`，会校验固定 npm 归档的
SHA-512，不运行包安装脚本，再将当前 Node 可执行文件复制到
`.tools/node`（Windows 为 `.tools/node.exe`）。语义分析运行时使用下列独立工具链：

| 工具或声明 | 固定版本 |
| --- | --- |
| typescript-language-server | 6.0.0 |
| TypeScript / tsserver | 6.0.3 |
| @types/node | 22.20.1 |
| undici-types | 6.21.0 |

固定包默认位于 `.tools/typescript-analyzer/`。如需自定义位置，请在启动应用前将
`NARRANT_TYPESCRIPT_NODE` 和 `NARRANT_TYPESCRIPT_RUNTIME` 分别设为 Node
可执行文件和固定包目录的绝对路径。应用构建使用的 TypeScript 版本仍由
`package.json` 和 `pnpm-lock.yaml` 管理，不会取代这套固定语义引擎。

Rust 分析继续使用开发仓库中的 `.tools/rust-analyzer`（Windows 为
`.tools/rust-analyzer.exe`），也可在启动应用前用 `NARRANT_RUST_ANALYZER`
指定可执行文件。分析期间不会下载工具、安装被审查仓库的依赖、运行项目构建或
执行项目插件。缺少或无法启动任一必需的语言服务都会使整次结构比较失败，不会返回
伪装成成功的空结果。

结构视图共同分析 `.ts`、`.tsx`、`.js`、`.jsx`、`.mts`、`.cts`、`.mjs`、
`.cjs` 及 `.d.ts`、`.d.mts`、`.d.cts` 声明文件。Oxc 提取语法结构和对象自身
内容，固定 TLS/tsserver 通过共享 LSP 提供语义证据。原生 JSDoc 推断会保留，
不使用另外的 JSDoc 解析器。直接绑定到 `const` 名称的箭头函数会成为 Callable；
内联回调保留在所属对象内容中，不产生匿名回调节点。自定义 JSX 组件会产生组件引用，
`div`、`button` 等内置标签不产生节点或组件关系；其属性中的表达式和回调仍参与
普通内容与关系分析。

默认语义上下文由仓库第一方源码、项目配置启用的标准库，以及固定 Node 声明闭包
组成，不包含业务第三方源码或类型。缺少业务依赖时，可靠关系会保留，无法确认的目标
保持未解析，并按前后两侧分别显示上下文说明。这套代码图是保守的静态语义结果，
不保证完整的 TypeScript 静态可靠性；输入路径检查和临时视图隔离也不构成操作系统级
沙箱。

生产夹具已覆盖现代 NodeNext、Bundler、JS/JSX、Promise、Node ESM/CJS 包装和
原生 JSDoc。TS 5 风格兼容夹具使用 CommonJS、旧式 Node moduleResolution、
baseUrl 和 paths，在固定 TS 6 引擎下保留可靠的本地调用，将冲突导入保持为未解析，
并显示原生弃用说明。这些代表性结果不表示使用项目安装的 TypeScript，也不构成对所有
TS 5/6 配置的兼容承诺。

上述工具链和真实语言服务验收在 macOS 上执行；Linux 和 Windows 尚未原生执行。
Windows 共享 LSP 代码和测试已通过交叉类型检查。正式安装包内分发分析工具仍是后续
工作。未配置文件的确定性默认规则及 workspace 支持边界见
[受控项目说明](src-tauri/src/structure_review/typescript_analyzer/runtime/README.md)，
产品范围和验收依据见
[TypeScript / JavaScript Code Graph 规范](docs/specs/0021-typescript-language-analyzer.md)。

## 构建 Windows 安装包

安装项目依赖后，在 Windows 系统的项目根目录运行：

```powershell
pnpm build:windows
```

该命令仅构建 NSIS `.exe` 安装包。构建产物位于：

```text
src-tauri\target\release\bundle\nsis\Narrant_1.0.0_x64-setup.exe
```

## 本地开发

Custom Babysit 的确定性 smoke、终端开发日志和数据保密边界见
[开发日志](docs/agents/dev-logging.md) 与 [Babysit smoke test](docs/agents/babysit-smoke.md)。

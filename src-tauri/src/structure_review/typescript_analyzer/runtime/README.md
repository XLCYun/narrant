# TypeScript 受控项目助手

这些模块在 Rust 编译时由 `include_str!` 内嵌，运行时写入独立 TempDir。
`prepare.mjs` 编排准备与严格 JSON 输出；`view.mjs` 复制第一方文件、校验路径、
保留包模块元数据并恢复第一方 workspace 链接；`config.mjs` 使用固定 TypeScript
6.0.3 读取 JSONC、解析 extends/references 和确定配置根文件。
助手不创建 Program/checker，不抽取代码图，不执行配置 JS、项目脚本或插件。

## 推断配置

现存 tsconfig/jsconfig 的配置语义优先。TS 配置解析给出初始根文件候选；
相对 paths 保留在原配置层，继承完成后按有效 baseUrl/pathsBasePath 校验范围。
LSP 注册后再通过显式 projectInfo 确认包含 imported/reference 源文件的实际项目。只有没有实际
配置项目的选中文件才使用下述推断 profile，并生成一条说明。

- Node 上下文判据为 `.cjs`、`.mjs`、`.cts`、`.mts`，或最近 package.json 存在
  `engines.node`、依赖/开发依赖 `@types/node`、`type: commonjs`。
  使用 NodeNext/NodeNext、ES2022、`lib.es2022.d.ts` 和固定 Node 22.20.1 声明。
- 其他未配置文件使用 Preserve/Bundler、ES2022、DOM/DOM.Iterable、JSX preserve，
  `types: []`。两个 profile 都允许 JS 并启用 checkJs。
- 这些是确定性的缺省规则，不代表对应用运行环境的自动检测。
  运行 TLS 的 Node 可执行版本绝不用于推断项目的 Node 类型版本。
- 已配置项目的显式 target/lib/noLib/types/typeRoots 保持权威，不强制 noResolve。
  缺失业务 extends/types 会得到说明或诊断，未提供的业务类型不会自动下载。

## 文件与工作区边界

仓库 node_modules 不作为源码输入；只有实际引用的配置 JSON 元数据可单独复制。
默认跳过 `.git`、`.tools`、`.narrant-typescript` 与 Cargo.toml 旁的 `target`。
一般第一方 `src/target` 之类目录仍会保留。第一方 TS/JS/声明和 JSON/JSONC 元数据
保留相对位置；Node 与 undici 的固定声明复制到视图 node_modules，避免 symlink
实路径导致 undici 解析跳出声明闭包。它们的证据 URI 会映回固定工具目录。

workspace 链接来自原有、真实目标仍在第一方目录中的 node_modules symlink，或
package.json workspaces 的字符串列表/`packages` 列表、pnpm-workspace.yaml 的简单
`packages` 字符串列表；支持 `*`、`**` 与前缀 `!` 排除。复杂 YAML、扩展 glob 不是
完整的包管理器实现；没有可确认的链接时保留未解析语义，不构造虚假的包源入口。
包 exports/imports/main/types 等元数据不会被替换成猜测的源码文件。

LSP 会话必须先注册，再打开文档，并对实际项目名和文件成员逐一校验。配置元数据
采用独立名单；第一方源码、第一方 JSON 与固定运行时声明之外的成员一律失败。

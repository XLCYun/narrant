# TypeScript / JavaScript Native Helper

固定 Node 24.18.0、TypeScript 6.0.3、@types/node 22.20.1、undici-types 6.21.0。
通过 `narrant.native-analyzer/1` 和 `typescript-static-v1` 一侧接收一次请求。
不运行项目代码、插件、构建、tsserver 或语言服务器。

- `workspace.mjs` 只允许固定侧的第一方源码/JSON 元数据和三套固定声明。
  workspace 包与 Node 声明通过内存文件接口解析，不创建 node_modules 链接。
- `config.mjs` 使用原生 JSONC 配置解析，保留 extends/references/configDir/paths、
  package imports/exports 与无 dist 的项目引用重定向。缺失业务配置沿用旧政策。
- `analyze.mjs` 为每个实际配置创建进程内 LanguageService；同一请求共享原生
  DocumentRegistry。语法树、诊断、`getDefinitionAtPosition` 均来自这些对象。
  checker 仅验证 type-only 路由，不能替换定义导航的 union 选择。
- `projection.mjs` 从原生 AST 收集声明；规范打印只裁剪声明成员列表，保留完整
  参数、初始化器和函数体。token 来自正确语法上下文的重新解析，保留 JSX、
  模板、正则、ASI、稀疏数组；不维护 Oxc 式的第二套 AST。
- 原生分析运行于当前 helper 进程内的单次 worker thread；主线程处理 cancel 和
  提前 stdin EOF，等待线程回收后 flush 唯一终结并退出。公共 Host 的 ProcessTree
  继续拥有整个进程树与超时清理；取消不能变成成功空图。
- Windows 的 stdout/stderr 使用公开回调式 `fs.write`，按字节补齐短写，避免标准
  stream 的同步管道写入阻塞控制。POSIX 保留 stream 背压处理。正常结果、错误与取消
  均等完整输出后 exit 0；提前 EOF 才允许丢弃余量。Windows 给已完成写入的回调
  100ms 收尾窗口，仍未 flush 才在分析线程回收后仅终止当前进程（exit 1），避免
  `process.exit()` 等待阻塞中的 libuv 写线程。诊断先于终结，正常 Host 可随终结关闭 stdin。

支持包构建与真实图 smoke：

```sh
node scripts/language-packs/build-language-support-packs.mjs --id typescript-javascript --target aarch64-apple-darwin --output native-language-support-packs
node scripts/language-packs/verify-native-analyzers.mjs native-language-support-packs --language typescript-javascript
```

Windows 使用 `x86_64-pc-windows-msvc`，在对应平台本机构建和验证。
聚焦验收包含 helper 自身、旧语言图断言、公共 Host/进程、真实安装、Review DTO、
真实取消与公开图/diff。所有 Cargo 命令由 `scripts/cargo-agent.sh` 串行执行。
单独运行 Node 测试需设置 `NARRANT_TYPESCRIPT_PACK` 为真实已解压包目录。

1.1.0 为迁移候选包，`published=false`；最终 GitHub-hosted macOS ARM64 / Windows x64
语言 Actions 已通过，见[迁移证据](../../docs/research/existing-native-helper-migration-validation.md)。
测试通过不会自动发布支持包。
旧 TypeScript 子树、TLS 开发安装链和 Oxc 宿主依赖已在本地 macOS 验收后退役；
身份、原文和包含树由固定历史断言继续验证，生产入口使用 Native Host。

别名选择沿用旧 TS 行为：按真实文件路径合并图节点，coverage 保留每个词法选集路径，
origins 聚合未被测试规则排除的来源。测试规则同时检查选中路径和真实仓库相对路径。
内部目录链接不扩大读取边界；越界链接、重复词法路径仍由公共 Host 拒绝。

macOS 性能样本（40 文件、400 函数/解析关系、三次独立进程，输出墙钟和 RSS）：

```sh
NARRANT_TYPESCRIPT_PACK=/absolute/extracted/pack node native-helpers/typescript/tests/benchmark.mjs
```

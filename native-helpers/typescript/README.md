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
- helper 不拥有额外进程管理器。同步编译期间的取消/超时由公共 Host 的
  ProcessTree 终止进程并等待回收；取消不能变成成功空图。

支持包构建与真实图 smoke：

```sh
node scripts/build-language-support-packs.mjs --id typescript-javascript --target aarch64-apple-darwin --output native-language-support-packs
node scripts/verify-native-analyzers.mjs native-language-support-packs --language typescript-javascript
```

Windows 使用 `x86_64-pc-windows-msvc`，在对应平台本机构建和验证。
聚焦验收包含 helper 自身、旧语言图断言、公共 Host/进程、真实安装、Review DTO、
真实取消与公开图/diff。所有 Cargo 命令由 `scripts/cargo-agent.sh` 串行执行。
单独运行 Node 测试需设置 `NARRANT_TYPESCRIPT_PACK` 为真实已解压包目录。

1.1.0 为迁移候选包，`published=false`；macOS/Windows Actions 通过前不发布。
旧 TypeScript 子树暂留用于身份/原文对照，生产入口只走 Native Host。

别名选择沿用旧 TS 行为：按真实文件路径合并图节点，coverage 保留每个词法选集路径，
origins 聚合未被测试规则排除的来源。测试规则同时检查选中路径和真实仓库相对路径。
内部目录链接不扩大读取边界；越界链接、重复词法路径仍由公共 Host 拒绝。

macOS 性能样本（40 文件、400 函数/解析关系、三次独立进程，输出墙钟和 RSS）：

```sh
NARRANT_TYPESCRIPT_PACK=/absolute/extracted/pack node native-helpers/typescript/tests/benchmark.mjs
```

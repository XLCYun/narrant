# TypeScript LSP 研究探针

用于复现 [LSP 验证报告](../../typescript-lsp-validation.md)，不是应用适配器或生产测试套件。
2026-09-14 在 macOS、Node 24.18.0 上运行；进程组检查使用 Unix 的 `ps`，没有验证 Windows。

从仓库根运行。需要 Node ≥22.22.2、`tar`，以及当前仓库已有的 TypeScript 5.9.3、
`@types/node` 22.20.1 和它的 `undici-types` 6.21.0。探针不安装被分析项目的依赖。
将来仓库升级这些依赖后，先提供对应固定版本再比较本次记录，不能冒用新版本结果。

```sh
node docs/research/probes/typescript-lsp/setup.mjs
node docs/research/probes/typescript-lsp/scope.mjs
node docs/research/probes/typescript-lsp/modules.mjs
node docs/research/probes/typescript-lsp/projects.mjs
node docs/research/probes/typescript-lsp/check-results.mjs
```

`setup.mjs` 从 npm 下载固定 TLS 6.0.0 与 TypeScript 6.0.3，校验 sha512 后解包，
不调用 npm install，不运行安装脚本。工具和 JSON 结果默认写到被 Git 忽略的
`.tools/typescript-code-graph-probe/`；可通过 `NARRANT_TS_PROBE_DIR` 指定独立目录。
分析 fixture 建在系统临时目录，以 JSON 结果中的 `root` 为准，不改真实仓库 tsconfig。

| 脚本 | 独立会话数 | 内容 |
| --- | --- | --- |
| `scope.mjs` | 8 | TS5/6；普通/受限项目；已有 tsconfig；先打开文档的反例 |
| `modules.mjs` | 7 | baseUrl/paths、Node16/NodeNext、JSX/JSDoc、受控 Node 类型 |
| `projects.mjs` | 8 | references 方案对照、workspace 包链接、共享文件的多项目归属 |
| `check-results.mjs` | 0 | 对已有结果执行 45 组断言，包括预期失败场景与进程回收 |

结果检查验证定义的目标文件及关键声明位置，不能用“响应非空”代替正确关系。
例如不可导出的包子路径会指回本文件的 import；这是需要保留 Unresolved 的反例。
`projects.mjs` 的两种 noResolve 模式预期丢失 references 跳转，检查通过不表示它们适合作为通用方案。

归档的 [结果索引](observations-2026-09-14.json) 是本次 23 个会话、45 组检查的名称与数量；
详细返回值、项目文件列表、路径、生命周期及 stderr 保存在本地 `*-results.json`。
如需重新生成详细证据，按以上命令顺序运行。源码生成逻辑就是固定的实验输入。

正常退出后会有界等待整个探针进程组；失败时只清理该探针创建的进程组。
这些实验没有覆盖应用取消链路、恶意项目、文件系统沙箱、跨平台进程回收或大型仓库性能。

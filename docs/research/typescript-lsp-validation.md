# TypeScript Code Graph：固定 LSP 服务的工程验证

日期：2026-09-14。范围是调研探针，未实现生产 Code Graph 适配器。
产品决策见 [设计记录](typescript-code-graph-plan.md)，集成工作见
[接入清单](typescript-integration-checklist.md)，服务源码约束见
[项目控制核查](typescript-lsp-project-control.md)。

## 结论

可以继续采用 **Oxc + typescript-language-server 6.0.0 + tsserver 6.0.3**。
LSP 包装层、标准定义查询与默认范围已获得真实服务证据；目前没有必要切换到自有
Compiler API 语义 worker。Oxc 的对象提取仍是后续实现，本轮没有跑完整 Oxc 适配器。

必须调整的工程设计是项目上下文：**不能把所有项目压成一个 roots 列表，再全局强制
`noResolve`。** 它在简单项目中有效，但实测破坏 project references 的源码跳转。
建议以保留项目结构的临时分析目录作为通用路线，在目录中仅提供本侧仓库代码、必要
配置/包元数据和允许的标准库/运行时声明，继续使用服务自身的模块解析及 references。
这增加配置投影、路径映射和目录准备成本，不要求另造类型检查器或 fork TLS。

“默认仓库内 + 标准库”仍指语义上下文范围；本次没有把它升级成对任意服务器输入的
操作系统文件访问沙箱。临时目录方案的通用实现、输入逃逸检测及大型仓库成本仍需验收。

## 实验基线与复现

| 项目 | 固定值 |
| --- | --- |
| 宿主 | macOS，Node 24.18.0 |
| LSP 服务 | typescript-language-server 6.0.0 |
| 语义引擎 | 推荐 6.0.3；scope 和 alias 同时对照仓库现有 5.9.3 |
| 标准库 | 对应语义引擎的 ES2022；JSX 样例另含 DOM |
| Node 类型探针 | `@types/node` 22.20.1 + `undici-types` 6.21.0 |
| 执行 | 23 个独立 LSP 会话，45 组结果检查全部符合预期，包括负例 |

TLS6 与 TS6 包从 npm 固定版本下载，经 sha512 校验后解包；没有安装项目依赖、运行
被分析代码或执行项目构建。脚本、固定 fixture 输入和复现步骤已归档到
[研究探针](probes/typescript-lsp/README.md)。JSON 摘要为
[本次检查索引](probes/typescript-lsp/observations-2026-09-14.json)。

Node 类型版本是本轮验证材料，不代表首版必须把所有项目强制解释为 Node 22。
正式运行时 profile 需要固定来源、版本与允许闭包，并在 before/after 使用同一约定。

## LSP 完整链路与范围

初始化使用固定 `tsserver.path`、`disableAutomaticTypingAcquisition: true`、
`plugins: []`、`tsserver.useSyntaxServer: "never"`。禁用独立 syntax server 是为了
避免项目加载期间定义请求落到不同服务实例；这由
[TLS 固定版本配置](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/docs/configuration.md)
支持。默认 UTF-16；所有查询经过实际 JSON-RPC stdio 帧。

最小项目控制顺序为：

1. initialize / initialized。
2. `workspace/executeCommand` 调用 `typescript.tsserverRequest`，执行 `openExternalProject`。
3. `textDocument/didOpen` 同步选中源码。
4. `projectInfo` 核验实际项目名、语言服务状态和文件范围。
5. 普通 `textDocument/definition`、prepare/outgoing call hierarchy。
6. shutdown、exit，等待进程组清空。

项目控制命令是 TLS 的
[公开扩展](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/commands/tsserverRequests.ts)，
没有修改服务源码。探针中的 standard definition 没有附带私有 project 参数，因而确实
验证了 didOpen 后的默认项目选择。

| 条件 | TS5.9.3 / TS6.0.3 的共同结果 |
| --- | --- |
| 普通 external project，允许自动加入 imports | 本地 TS、JS 调用及第三方成员全部解析；第三方 `.d.ts` 进入 projectInfo |
| 完整显式 roots + noResolve，先注册项目再 didOpen | Promise + barrel 后的本地方法、JS require 后的本地方法都正确；第三方不在 projectInfo，第三方方法定义为空 |
| 仓库有真实 tsconfig，但仍先注册上述 external project | didOpen 后仍使用预期 external project；结果同上 |
| 先 didOpen 再注册受限 external project | 后续查询可切换，但先前 configured project 已加载第三方；不符合首次加载就受控的顺序要求 |

受限简单 fixture 包含 4 个仓库源文件与 57 个 ES2022 库文件；普通模式再多 1 个第三方声明。
这是项目成员证据，不是磁盘读取审计或性能基准。

`noResolve` 的正式语义是阻止 imports / reference 指令扩充 Program 文件集合，仍会做
模块解析，不能解释为“所有模块都不解析”。
[TypeScript 选项说明](https://www.typescriptlang.org/tsconfig/noResolve.html)。

## 模块、运行时与已有语言能力

| 样例 | 实测结果与接入含义 |
| --- | --- |
| baseUrl + `@/*` paths + barrel | TS5.9/6.0 都定位到原始类方法；TS6 同时报告 baseUrl 弃用诊断 5101，不能把它误报成已删除或必须升级项目 |
| Node16 / NodeNext 的 package imports 与 self exports | `.mts/.mjs` 命中 import 分支，`.cts/.cjs` 命中 require 分支；目标文件不同且正确 |
| TypeScript 源码导入 `./relative.js` | 正确定位到 `relative.ts` |
| exports 明确公开与未公开的子路径 | 公开路径定位正确；未公开路径没有落到私有文件，而是指回本地 ImportSpecifier，必须保留 Unresolved，不能见非空 definition 就建调用边 |
| JSX `<Card />` | 定义命中 const Card，Page 的 outgoing hierarchy 包含 Card；需按 AST occurrence 分类为组件使用 |
| JSX `<div>` / `<button>` | Page 样例的 outgoing hierarchy 没有 HTML 标签；图提取仍需明确排除 intrinsic 对象/关系，保留原文内容 |
| `const chain = items => items.map(item => item.save())` | hierarchy 可从 chain 看到回调内部的 save；不建匿名回调节点也能保留内部关系 |
| JS 的 JSDoc `@param {import("./model").Service}` | 原文直接交给服务，`item.save()` 定位到仓库 model.ts；没有专用 JSDoc 适配 |
| `.js` 中 JSX | JS 的 `<JsCard />` 定义查询成功；Oxc 解析模式仍需显式对齐 |

Node 标准上下文另做开关对照：

```ts
import { AsyncLocalStorage } from "node:async_hooks";
export class Service { save() { return "ok"; } }
const storage = new AsyncLocalStorage<Service>();
export function current() { return storage.getStore()?.save(); }
```

仅 ES2022 时，最后一个 save 无目标。显式提供固定 Node 声明及 undici-types 后，
它指向仓库内 Service.save；`node:fs/promises` 的 readFile 和 Buffer.toString 也正确
定位到允许的运行时声明。运行时声明会影响本地关系的恢复，不能简单认为只影响外部边。
本轮通过显式提供 Node 类型集验证能力，尚未实现自动选择/准备该类型集的发行逻辑。

另一个数据清洗点：TS5/6 的简单样例里，部分 outgoing call 的 `fromRanges` 重复。
必须按 occurrence/关系事实去重；不能将返回条数或临时位置直接参与 Code Graph diff。

## 多项目与 project references

fixture 有 app 和 composite lib。lib 的 package.json 指向 `dist/index.d.ts`，
实际没有 dist；app 引用 lib 的 tsconfig，并调用 lib/src/index.ts 内的方法。

| 方案 | 仓库调用 | 第三方范围 | 判断 |
| --- | --- | --- | --- |
| 原始 configured project | 正确跳到 lib 源码 | 加载已安装第三方声明 | 对照组，不能作为默认范围方案 |
| 一个 external project，显式 app/lib 源码 roots + noResolve | 无目标 | 第三方不进入项目 | 不能作为通用 references 方案 |
| 保留受控 tsconfig/references，并给 configured project 设置 noResolve + 完整 files | 仍无目标 | 第三方不进入项目 | 仅保留 references 字段不足以修复 |
| 临时分析目录保留配置/refs/package 元数据，省略第三方源码声明，不强制 noResolve；通过 paths 导入 lib | 正确跳到 lib 源码 | 第三方不进入项目 | 样例通过 |
| 同一目录策略，按包名导入，并在分析目录创建指向仓库 lib 的 workspace 链接 | 正确跳到 lib 源码 | 第三方不进入项目 | 样例通过；不需要安装项目依赖 |

TypeScript 的 project references 包括声明产物与编辑器源码重定向语义，不能仅等价成
“把另一个目录的文件加进来”。官方也说明 VS Code 使用源码重定向缓解缺构建产物场景。
[Project References 文档](https://www.typescriptlang.org/docs/handbook/project-references.html)。

因此建议首版使用一个明确的项目上下文模块，统一准备保留目录关系的分析输入。
`noResolve` 路线保留为简单样例和回归对照，不安排第二套自动切换的生产策略。
临时分析目录的验证本轮由 fixture 构造完成，尚没有实现任意真实仓库的投影器。

同一文件属于多个项目也已验证：共享文件通过 `@impl` 导入，A/B 的 paths 分别指向
不同实现。一个服务注册两个 external project 时，普通 definition 选择默认 A；显式
tsserver projectFileName 可得到 B，分别只注册 A/B 的独立会话也各自正确。

适配器不能把默认 A 当成该文件唯一含义。需记录实际配置归属；正常单上下文可使用标准
LSP，多上下文使用明确的项目查询或受控会话。若同一源码 occurrence 在有效上下文之间
得到相互冲突的目标，保留不确定性和运行说明，不产生多个“确定调用”，也不复制源码节点。
私有查询必须核验实际 projectInfo 名称；无效 projectFileName 可能回落默认项目，详见
[项目控制源码约束](typescript-lsp-project-control.md)。

## 通用分析目录需要承担的工作

这是后续实现要求，不能据小样本已经成功就声称全部完成：

1. 从当前侧受保护的 repository root 读取输入，保留仓库相对布局；源码原样提供，绝对
   服务 URI 映回本侧仓库路径。稳定 NodeId 不含临时目录，不另建第二个 Git head worktree。
2. 保留有效 tsconfig/jsconfig、extends、references、模块模式、paths、package imports/exports。
   只投影范围控制所需配置，关闭 ATA、项目插件和业务自动 types；不能把各项目统一改成 Bundler。
3. 允许 ECMAScript/DOM 库及明确的 Node 类型闭包。声明输入按来源判定，不能按 `.d.ts`
   后缀一律拒绝；仓库声明文件和标准库都需要保留。
4. 从仓库 manifest 识别一方 workspace 包，重建只指向分析目录内部的链接；不能复制
   当前 checkout 的整个 node_modules，不能用猜测入口覆盖包的 exports 限制。
5. 处理配置中的绝对路径、跨根 symlink、三斜线指令、标准库替换和包配置继承等入口。
   所有有效路径都应映入允许上下文；无法确定的外部来源必须报出局限/错误，不借原 checkout 补齐。
6. 在 didOpen 前注册受控项目；按具体项目响应检查实际名、可用性和成员，异常扩大范围
   不得作为成功结果继续。projectInfo 校验用于发现错误配置，不冒充事前文件系统沙箱。
7. 临时目录由实际分析 worker 持有并清理，沿用 Review Lifecycle 的 lease 与取消范围；
   不改变已有 before/head 选择或 workspace 所有权。

这里包含需要实测的目录准备开销。应记录仓库文件数、准备时长、服务加载时长和峰值内存，
再决定是否使用文件复制优化；本轮不提供没有测量依据的人天、延迟或内存承诺。

## 生命周期、诊断与尚未验证的部分

23 个会话都接受 `shutdown` 的 `params: null`，收到响应后发送 exit，TLS 进程以 0
退出；有界等待后，属于该探针的进程组为空，stderr 为空。早期检查中出现过瞬间尚未
回收的子进程，因此最终检查显式等待，而没有把 shutdown ack 当成整棵进程树已退出。
服务源码本身的关闭/取消约束见 [源码核查](typescript-lsp-project-control.md)。

这些结果不能替代应用级取消、初始化失败、服务超时、worker panic、Windows 子进程
回收验收。TLS 项目扩展也不能只依赖 `$/cancelRequest` 取消底层工作；实际进程宿主
必须能取消整次分析并等待子进程和分析目录清理完毕。

仍须完成的生产工作：Oxc 节点/自身内容提取、UTF-8↔UTF-16 映射、真实仓库配置投影、
完整 TS5.0/5.9/6.0 兼容矩阵、缺失/陈旧声明产物、标准库 profile、动态目标和多声明
映射、应用路由/coverage/报告、图合并、取消与性能、前端验收。
已有服务原生能力由本轮验证支撑，不等于上述适配器工作已经交付。

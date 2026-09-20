# TypeScript LSP 受控项目接入核验

日期：2026-09-14（Asia/Shanghai）。核验范围固定为
`typescript-language-server v6.0.0`（下称 TLS）和 `TypeScript v6.0.3`。
本文是官方协议与固定版本源码的静态审计，不包含自行运行 LSP 的结果。
本轮实际探针、TS 5.9.3 对照及产品方案见[接入方案与验证记录](typescript-code-graph-plan.md)。

目前公开接口足以注册受控项目、检查项目成员及指定查询项目，不需要先决定 fork TLS。
主要边界是配置与文件可见性：普通 external project 不保留 project references；
需要 references 的仓库应验证保留配置结构的受控分析目录。
`all roots + noResolve` 只能作为受限方案，不能概括为完整的多项目支持。

## 固定运行时与服务器路由

TLS 6.0.0 声明 Node.js `>=22.22.2`，通过 Node 子进程运行选定的 tsserver。
初始化应显式设置 `tsserver.path` 指向随附且校验过的 `tsserver.js`；
`fallbackPath` 只在其他发现路径不可用时生效，不能用来固定语义版本。
[TLS package.json](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/package.json#L12-L21)、
[path / fallbackPath 定义](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/docs/configuration.md#tsserver-options)、
[子进程启动](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/tsServer/serverProcess.ts#L21-L44)

建议本轮接入使用以下初始化片段，具体安装路径由应用提供：

```json
{
  "initializationOptions": {
    "disableAutomaticTypingAcquisition": true,
    "plugins": [],
    "tsserver": {
      "path": "/application/runtime/typescript/lib/tsserver.js",
      "useSyntaxServer": "never"
    }
  }
}
```

`useSyntaxServer: "never"` 使用单个完整服务器。默认 `auto` 在 TS 6 下启动 syntax、
semantic 两个服务器，项目加载期间可能将 definition、references 等查询路由至 syntax。
源码虽有 `Always` 枚举，但标记为 unused，字符串转换没有启用 `"always"`；公开配置应只用
`auto` 或 `never`。
[启动模式](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/tsServer/spawner.ts#L48-L107)、
[动态查询路由](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/tsServer/server.ts#L417-L499)、
[配置转换](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/utils/configuration.ts#L56-L69)

关闭 Automatic Type Acquisition 只禁止自动取得缺失的 `@types`，不会禁止读取已有依赖。
初始化的空 `plugins` 也不能代替受控配置审核；configured project 还有读取
`compilerOptions.plugins` 并尝试启用插件的路径。分析目录应按已确认范围提供源码、
标准库和运行时类型，保留必要包元数据，并消除指向原仓库依赖或非授权插件的入口。
[ATA 设置定义](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/docs/configuration.md#initializationoptions)、
[configured project 插件入口](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/project.ts#L3082-L3105)

## 公开的 tsserver 请求通道

TLS 提供 `workspace/executeCommand`，命令为 `typescript.tsserverRequest`，参数按顺序为
`[tsserverCommand, protocolArguments, executeInfo]`。
`executeInfo.executionTarget` 的 `0` 表示 semantic，`1` 表示 syntax；
`expectsResult` 默认 `true`，`isAsync` 默认 `false`。
[公开命令文档](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/README.md#send-tsserver-command)、
[枚举定义](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/tsServer/server.ts#L24-L27)

文档写 execution target 默认是 semantic，但 `executeCustom` 的默认对象实际只填
`expectsResult` 和 `isAsync`，没有填 target；因此项目控制和明确的语义 raw query
应显式给 `executionTarget: 0`。`open`、`close` 等不产生 tsserver 响应的命令必须使用
`expectsResult: false`；`openExternalProject`、`closeExternalProject`、`projectInfo`
可以等待响应。命令结果保留 tsserver response 结构，不能一律当作 `body` 数组。
[executeCustom 实现](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/ts-client.ts#L556-L576)、
[external project 响应处理](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/session.ts#L3409-L3422)

通道只对参数顶层的 `file` 尝试转换已打开文档 URI；不会递归改写 `rootFiles`、
`projectFileName` 或 `options.paths`。项目注册应直接使用分析目录中的绝对文件路径。
[参数转换实现](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/commands/tsserverRequests.ts#L27-L47)

## ExternalProject 的两种行为

| 注册内容 | TS 6.0.3 的处理 | 适用边界 |
| --- | --- | --- |
| `rootFiles` 只有源码/声明文件 | 创建真正的 ExternalProject；使用传入的 roots、options、typeAcquisition | 适合明确掌握输入与单份选项的受限项目 |
| `rootFiles` 包含实际存在的 tsconfig/jsconfig | 创建或关联 ConfiguredProject；登记 external 名称到配置项目的管理映射 | 配置内容、extends、references 和依赖解析继续由配置项目负责 |

两种行为不是选项叠加。进入 configured 分支后，同名真正 external project 会被移除；
传入 external 的普通源码 roots、`options`、`typeAcquisition` 不会覆盖这些配置项目。
限制必须落实在受控配置与其可见文件中。
[分支实现](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/editorServices.ts#L5344-L5410)

例如，可在任何 `didOpen` 之前注册已构建好的受控配置：

```json
{
  "command": "typescript.tsserverRequest",
  "arguments": [
    "openExternalProject",
    {
      "projectFileName": "/analysis/.narrant-projects",
      "rootFiles": [
        { "fileName": "/analysis/apps/web/tsconfig.json" },
        { "fileName": "/analysis/packages/lib/tsconfig.json" }
      ],
      "options": {}
    },
    { "executionTarget": 0, "expectsResult": true, "isAsync": false }
  ]
}
```

这里 `/analysis/.narrant-projects` 是注册批次的管理名称。
后续指定语义项目时应使用真实的 `/analysis/apps/web/tsconfig.json` 等配置路径。
服务器的 `findProject` 只查找 external 实体和 configured 实体，不用该管理映射选择某个配置项目。
[项目查找](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/editorServices.ts#L1787-L1795)

`openExternalProjects` 是同步完整列表的操作：未出现在新列表中的旧 external 注册会被关闭。
不能将它当成向现有集合追加一个项目。
[批量注册实现](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/editorServices.ts#L5193-L5214)

## References 与 noResolve 的真实限制

协议 `ExternalProject` 只有名称、roots、options、typeAcquisition，没有 `projectReferences`。
普通 ExternalProject 继承的 `getProjectReferences()` 返回 `undefined`，也没有 configured
project 用于源码重定向的相同宿主覆盖。
[协议结构](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/protocol.ts#L1390-L1420)、
[基类默认值](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/project.ts#L694-L696)、
[ExternalProject 实现](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/project.ts#L3162-L3202)

ConfiguredProject 保存解析出的 references，并提供 `getParsedCommandLine` 和
`useSourceOfProjectReferenceRedirect`。编译器还受
`disableSourceOfProjectReferenceRedirect` 控制。
这说明源码重定向依赖项目结构，不能仅凭“lib/src/index.ts 也在 roots 中”推断
`package.json` 指向但尚不存在的 `dist/index.d.ts` 必然能回到源码。
[配置项目宿主](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/project.ts#L2964-L2984)、
[配置项目 references](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/project.ts#L3053-L3059)、
[编译器重定向条件](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/compiler/program.ts#L1717-L1733)

`noResolve` 在导入扩展 Program 文件时有整体阻断条件，不能按第一方、第三方选择性放行。
显式 roots 可以补回某些文件，但不能据此声称保持配置引用、声明输出路径及模块解析的全部语义。
即使保留 references，也不能推断强制 `noResolve` 不影响源码重定向。
另外，Program 文件名单不等于所有文件系统读取记录；模块解析仍可能读取 package 元数据。
[导入文件加入 Program 的条件](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/compiler/program.ts#L3940-L3968)

因此，本轮应优先验证“保留项目结构的受控分析目录 + configured project”。目录需要保留
相对层级、配置继承、references、必要 package 元数据与仓库 workspace 的映射，
并按允许范围提供实际源码和类型。该方案仍用公开 LSP 扩展；它是否完整满足不同仓库，
由探针结果决定，不能由注册成功推导。

## didOpen 与查询究竟选择哪个项目

TLS 的 `didOpen` 会发送 tsserver `open`，包括绝对路径、文本、ScriptKind 和 projectRootPath。
标准 definition、prepareCallHierarchy、incoming/outgoing 查询只提供文件与位置，
不携带 `projectFileName`；call hierarchy item 也没有在后续请求中恢复此前项目选择。
[文档打开](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/document.ts#L279-L311)、
[definition 参数](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/lsp-server.ts#L518-L550)、
[call hierarchy 参数](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/lsp-server.ts#L1270-L1307)

TS 6.0.3 对同时属于多个项目的文件，优先选择第一个 external project；随后考虑打开文件时
确定的 configured project、不是 reference redirect 源的 configured project、其他 configured
project，最后是 inferred project。“当前文件旁边有 tsconfig”不意味着普通 LSP 一定选它，
“最后注册 B”也不意味着查询改选 B。
[默认项目选择顺序](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/scriptInfo.ts#L575-L623)

raw query 可以显式传 `projectFileName`。定义、类型定义等位置查询使用的
`FileRequestArgs` 有此字段；definition 和三项 call hierarchy 的服务端实现都会取该项目。
但名称无效时 `getFileAndProjectWorker` 会回落到默认项目，所以必须核对返回的项目名。
[协议字段](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/protocol.ts#L338-L348)、
[选择与回落](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/session.ts#L2315-L2330)、
[definition 实现](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/session.ts#L1612-L1616)、
[call hierarchy 实现](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/session.ts#L3363-L3385)

例如，以下 `workspace/executeCommand` 参数可验证指定项目：

```json
{
  "command": "typescript.tsserverRequest",
  "arguments": [
    "projectInfo",
    {
      "file": "/analysis/apps/web/src/main.ts",
      "projectFileName": "/analysis/apps/web/tsconfig.json",
      "needFileNameList": true
    },
    { "executionTarget": 0, "expectsResult": true, "isAsync": false }
  ]
}
```

应检查 `result.body.configFileName` 与请求的配置路径一致、
`languageServiceDisabled` 为 false，并检查 `fileNames` 的范围。
`projectInfo` 会更新脏项目，且这里的文件列表包含 external library 文件，没有先过滤它们。
所以它适合检查 Program 成员；它不能证明整个进程从未读取其他路径。
[projectInfo 实现](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/session.ts#L2075-L2090)

多份配置共享文件时，可选择每个有效查询项目独立会话，或让必要查询通过扩展显式选项目。
后者要转换 tsserver 的位置和响应，并把项目身份传到后续 hierarchy 请求；
仅给标准 LSP 的 params 临时加一个同名字段不会生效。

## 请求顺序、取消与回收

建议的会话顺序是：准备完整受控目录；初始化固定服务器；等待 external/configured 注册响应；
再 `didOpen`；用 `projectInfo` 验证默认及指定项目；最后查询。普通 external 分支会立即
更新图，源码注释明确说明后续打开文件需要更新后的 external 项目。
默认双服务器模式下，`open` 会广播，`openExternalProject` 不在广播集合中，
这也支持当前选用单服务器的接入方式。
[external 更新顺序](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/server/editorServices.ts#L5383-L5406)、
[共享命令集合](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/tsServer/server.ts#L327-L350)

标准语义查询传递 LSP cancellation token；但 `typescript.tsserverRequest` 进入
command manager 时只传命令参数，没有把 `workspace/executeCommand` 的 token 注入
`executeCustom`。当前版本不能假定 `$/cancelRequest` 会取消正在执行的 raw tsserver 查询。
宿主仍需提供任务截止时间与整个会话的取消回收。
[executeCommand 分支](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/lsp-server.ts#L958-L961)、
[raw 请求执行](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/ts-client.ts#L556-L576)

TLS 的连接注册未将 `LspServer.shutdown` 绑定到 `onShutdown`；初始化时注册的进程
`exit` 回调会调用 shutdown，继而调用子服务器 `kill()`。最终是 `ChildProcess.kill()`，
没有等待 child exit 的 Promise。TS 6 的 IPC 子进程另有 disconnect 后退出的路径。
因此应完成 LSP `shutdown` → `exit`，等待 wrapper 退出，并在有界时间内确认自有子进程退出；
单独看到 shutdown 响应、或者立即看到一条残留进程记录，都不足以判断回收结果。
[LSP 连接注册](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/lsp-connection.ts#L17-L67)、
[进程退出回调](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/lsp-server.ts#L217-L234)、
[tsClient.shutdown](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/ts-client.ts#L459-L466)、
[child kill](https://github.com/typescript-language-server/typescript-language-server/blob/v6.0.0/src/tsServer/serverProcess.ts#L105-L119)、
[TS IPC disconnect](https://github.com/microsoft/TypeScript/blob/v6.0.3/src/tsserver/nodeServer.ts#L588-L595)

后续工程验收应围绕已发现边界收敛：未构建的 composite 包能否重定向；共享文件能否按配置
稳定查询；第一方 workspace 的路径与包入口能否保留；第三方类型是否实际未进入 Program；
取消、超时和正常退出是否完整回收。静态接口可用不代表这些行为已经全部通过。

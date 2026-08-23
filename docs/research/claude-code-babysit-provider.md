# Claude Code 作为宝宝模式 Provider 的实现调研

> 调研日期：2026-08-23  
> 范围：只评估实现方式，不修改业务代码。仓库结论来自当前源码与规格；Claude
> Code 结论只引用 Anthropic 官方文档或官方仓库。

## 结论摘要

> **后续调研修订**：本文最初建议每轮新进程并用 `--resume` repair；Q3 证实这会依赖
> 本地 transcript，与“分析 session 不持久化”冲突。若零持久化是硬要求，最终推荐改为
> fresh-session repair 并更新 same-thread 内部决策；若 same-session 也是硬要求，则需
> Anthropic 批准 Agent SDK subscription 集成后使用单个 streaming-input SDK call。
> 下文早期 CLI 方案保留为能力分析，不再代表最终默认路线。

Claude Code 能满足宝宝模式的核心生成契约，但不能在完全不改变现有产品约束的
情况下直接替换 Codex App Server。

建议首版采用 **installed CLI adapter**，每一轮启动一次非交互进程，通过
`--resume <session_id>` 在 repair 时续接，而不是依赖文档不完整的双向
`--input-format stream-json` 协议。生成结果优先使用 `--output-format json` 加
`--json-schema`，由 Claude 做第一层结构约束，再继续执行 Narrant 已有的严格
coverage、安全字符与长度校验。Claude CLI 的非交互模式、JSON Schema 输出和精确
session 续接均有正式文档支持。[CLI reference](https://code.claude.com/docs/en/cli-usage)
[Programmatic usage](https://code.claude.com/docs/en/headless#get-structured-output)

不过，上线前必须先决定三个产品/安全问题：

1. **认证路线**：可重复的 `--bare` 模式不读取订阅 OAuth 或系统 keychain，只支持
   API key、`apiKeyHelper` 或云厂商凭据；复用用户 Claude 订阅则不能用 `--bare`。
   [Programmatic usage: bare mode](https://code.claude.com/docs/en/headless#start-faster-with-bare-mode)
2. **隔离强度**：CLI 工具白名单可以移除写入、Shell、Web 和 MCP 工具，但这不是
   Codex read-only sandbox 的等价 OS 安全边界；`--safe-mode` 仍保留 managed policy
   中的部分 hook。若规格中的“只能读当前 workspace、不能联网”是强安全承诺，需在
   CLI 外增加平台 sandbox，或把 API-key-only `--bare` 定为前置条件。
   [CLI reference: `--safe-mode`, `--tools`](https://code.claude.com/docs/en/cli-usage)
3. **会话清理**：Claude CLI 支持恢复和禁用持久化，却没有官方的“删除一个普通
   session transcript”命令。`--no-session-persistence` 又会使跨进程 repair 无法
   `--resume`。因此现有“所有宝宝模式 thread 都从历史永久删除”的要求必须调整，
   或改走经过验证的单进程/SDK 会话方案。
   [CLI reference: session flags](https://code.claude.com/docs/en/cli-usage)

## 仓库现状与改造边界

### 已经可复用的 provider-independent 核心

现有规格明确要求 Codex、Claude Code 和未来内置 provider 满足同一个外部契约，
且 provider 协议不得泄漏到 Review Reader。ADR 也已确定 provider identity 参与缓存，
新增 provider 应复用同一行为测试套件。
[Babysit spec](../specs/0012-babysit-mode.md#provider-decisions)
[ADR-0021](../adr/0021-separate-babysit-agent-providers-behind-one-contract.md)

后端 trait 已具备 Claude adapter 所需的主要形状：

- `provider_id` 与 `process_generation` 隔离缓存；
- `generate` 接收完整 before/after File Change，禁止逐行请求；
- `repair` 要求在同一 provider 会话中修复；
- `abandon` 和 `shutdown` 提供取消与清理 seam。

来源：[`BabysitAgentProvider`](../../src-tauri/src/babysit/mod.rs#L374)。上层已经统一
拥有三次 repair、十分钟总期限、结果 coverage 校验、进程内缓存和 provider-independent
进度，不应在 Claude adapter 中复制这些策略。
[Babysit orchestration](../../src-tauri/src/babysit/mod.rs#L525)

### 目前仍是 Codex 专用的外围 seam

生产装配并非真正多 provider：`BabysitApplication` 同时保存通用 provider 和
`codex_runtime`，生产构造器固定为 `with_codex_provider_at`；runtime prepare、可执行
文件选择与 shutdown 也直接分支到 Codex。
[Application assembly](../../src-tauri/src/babysit/mod.rs#L459)
[Tauri bootstrap](../../src-tauri/src/tauri_app/bootstrap.rs#L71)

此外，领域错误、IPC error code、recovery action、前端类型和文案都只有
`CodexNotFound`、`CodexSignedOut`、`LoginCodex`、`SelectCodexExecutable` 等 Codex
分支。[Backend errors](../../src-tauri/src/babysit/mod.rs#L210)
[Frontend errors](../../src/babysit/types.ts#L10)

因此实际改造至少需要：

1. 将 discovery/auth/readiness/executable picker/shutdown 提升为 installed-provider
   runtime 接口，而不是继续给 `BabysitApplication` 增加 `claude_runtime` 字段；
2. 引入 provider selection，并让选择进入请求 identity、缓存 key 和诊断；缓存 key
   已含 `provider_id`，但当前前端请求没有 provider；
3. 把安装、登录、升级、选择可执行文件恢复动作改为携带稳定 provider identity 的
   通用结构，UI 再映射为 Codex 或 Claude 文案；
4. 为 Claude adapter 复用 provider contract tests，并新增 CLI fixture 覆盖 JSON
   result、repair resume、取消、超时、认证失败、进程失败和未知事件。

## Claude Code 能力矩阵

| 能力 | 官方能力 | 对宝宝模式的含义 |
| --- | --- | --- |
| 非交互 | `claude -p` 执行一次请求后退出；成功为 0，失败非 0。启动参数错误写 stderr，运行内错误（包括认证）作为 result 写 stdout。 | 使用普通子进程与 pipe，不需要 PTY；业务终态同时检查 result 与 exit code。 |
| 输入 | prompt 可从 argv 与 stdin 输入；管道 stdin 上限 10 MB。 | 固定指令放 argv，完整文件内容走 stdin，避免 Windows 命令行长度/转义问题；超过 10 MB 应映射为可见 provider limit。 |
| 结构化输出 | `--output-format json --json-schema <schema>`，结构值位于 `structured_output`。 | schema 描述 `before`/`after` map；Narrant 仍负责精确 target coverage 和 240 字符规则。 |
| 流式输出 | `stream-json` 是 NDJSON；末行是 `result`；token delta 另需 `--verbose --include-partial-messages`。 | 当前 UI 只展示 generating/repairing/completed，不需要 token delta；首版可用普通 JSON 降低协议面。若使用流式，必须忽略未知事件并只记录脱敏元数据。 |
| 会话续接 | JSON/result 提供 session ID；`--resume <id>` 精确恢复；`--continue` 只按目录找最近会话；`--session-id <UUID>` 可预置。 | 首轮由 Narrant 预置 UUID 或读取 result ID；repair 必须用精确 `--resume`，不能用 `--continue`。 |
| 取消 | `SIGTERM` 退出 143，留下未完成 turn 且无 result；`SIGINT` 表示结束 turn，语义不同。 | 先用有界 SIGINT 尝试结束 turn，再 SIGTERM/kill process tree；任何取消输出都不得缓存。恢复一个 SIGTERM 中断的 session 会继续未完成 turn，不能误用于 repair。 |
| 权限 | `--tools` 才是 built-in tool 白名单；`--allowedTools` 只是免提示，不是白名单。`dontAsk` 会拒绝未允许动作。MCP 不受 `--tools` 影响，需单独 deny。 | 建议最小集合 `Read,Glob,Grep`，同时 `--disallowedTools "mcp__*"` 与 `--permission-mode dontAsk`；绝不使用 bypass/dangerously-skip。 |
| 工作目录 | primary workspace 是启动进程的 cwd；`--add-dir` 会扩大可读写范围。 | 父进程显式设置 Review Workspace 为 cwd，禁止 `--add-dir`。 |
| 认证 | 支持云厂商、auth token、API key、helper、profile、OAuth 等，存在明确优先级；`claude auth status` 可探测。 | 只执行官方状态检查并分类错误，绝不读取 credential 文件或 token。 |
| 模型 | `--model` 接受 alias 或完整 ID，`--effort` 设置工作量。 | 首版宜显式选择产品批准的 alias/effort；alias 会漂移，完整 ID 会退休，应纳入配置决策。 |

能力来源：

- [Programmatic usage: basic usage and exit semantics](https://code.claude.com/docs/en/headless#basic-usage)
- [Programmatic usage: structured and streaming output](https://code.claude.com/docs/en/headless#get-structured-output)
- [Programmatic usage: stop with SIGTERM](https://code.claude.com/docs/en/headless#stop-a-run-with-sigterm)
- [CLI flags](https://code.claude.com/docs/en/cli-usage#cli-flags)
- [Permissions](https://code.claude.com/docs/en/permissions)
- [Authentication precedence](https://code.claude.com/docs/en/authentication#authentication-precedence)

## 推荐的首版进程协议

### 生成轮

概念参数如下；实现时必须通过 argv 数组传递，不能拼 shell 字符串：

```text
claude -p
  --output-format json
  --json-schema <babysit-schema>
  --session-id <narrant-owned-uuid>
  --permission-mode dontAsk
  --tools Read,Glob,Grep
  --disallowedTools mcp__*
  --model <approved-model>
  --effort low
  <generation-instruction>
```

父进程设置 cwd 为 Review Workspace，并把 before/after、target inventory、locale 通过
stdin 发送。`--json-schema` 只约束 JSON 形状；动态行号 map 可用
`additionalProperties: {type: "string"}`，然后交给当前 `validate_response` 校验精确
key 集合。

认证/隔离参数按最终决策分两档：

- **API key / cloud credential 档（推荐）**：增加 `--bare`，避免加载 CLAUDE.md、hooks、
  skills、plugins、MCP 和 memory。官方说明 bare 不读取 OAuth/keychain，需用户自己的
  API key、`apiKeyHelper` 或云凭据。
- **复用官方 CLI 登录档**：增加 `--safe-mode` 以关闭多数 customization，并保留官方
  OAuth；但 managed policy 的部分 hook 仍可能执行，因此若坚持强隔离承诺，需要
  外层 OS sandbox。不能把 safe mode 描述成安全边界。

`-p` 默认会加载用户/项目 customization，并且不会显示 workspace trust 或 MCP
approval，因此无论哪档都不能裸用默认 `claude -p`。
[Bare-mode warning](https://code.claude.com/docs/en/headless#start-faster-with-bare-mode)

### Repair 轮

repair 启动新进程，保持同一 cwd、模型、权限与 schema 参数，增加
`--resume <narrant-owned-uuid>`，prompt 只包含 Narrant 的验证失败摘要与重新输出要求。
不要把上一轮完整输出再次放入 prompt；它已经在 session 中，也避免扩大敏感数据面。

不推荐首版用 `--continue`，因为它按“当前目录最近会话”寻址，存在并发和异常退出串线
风险。不推荐把 `--input-format stream-json` 作为关键依赖：CLI reference 列出了该
flag，但官方 programmatic guide 没有完整定义双向输入 envelope、permission control
与多 turn 关闭协议。若后续需要单进程多 turn，应直接评估官方 Agent SDK 的 typed
messages、interrupt 和 permission callbacks。
[Agent SDK positioning](https://code.claude.com/docs/en/headless)

### 输出与错误分类

解析器应按以下顺序收敛结果：

1. 总期限或 cancellation 先终止进程树，返回 provider-independent timeout/cancelled；
2. stdout JSON 必须有终态 result，并解析 `structured_output`；
3. result 显示认证、模型、额度或 permission denial 时映射为稳定 Claude-specific
   setup error；
4. 非 0 exit、缺 result、无效 JSON、schema 缺失统一进入 provider/protocol error；
5. stderr 只用于启动错误分类与脱敏诊断，不作为唯一业务判据。

若未来改为 `stream-json`，使用逐行有界 parser，忽略未知事件；`system/init` 可读取
model/tools 等 capability metadata，`system/api_retry` 可用于内部进度，但这些事件
不得跨越现有 generating/repairing/completed UI seam。官方说明 result 是流的末行，
API retry 也有稳定事件形状。
[Streaming](https://code.claude.com/docs/en/headless#stream-responses)
[API retries](https://code.claude.com/docs/en/headless#handle-api-retries)

## 安全、认证与合规

Anthropic 官方条款允许产品运行**未修改**的 Claude Code binary，但每个最终用户必须
使用自己的 API key、Claude subscription 或第三方云凭据，产品方不得代付、转售或
中间代理用量。第三方应用不得内嵌 Claude.ai 登录、代表用户路由 Free/Pro/Max
credential，也不得收集、保存或中转 Claude.ai credential/session token；Claude
account 登录必须通过 Anthropic 自有流程完成。
[Legal and compliance: products and credentials](https://code.claude.com/docs/en/legal-and-compliance#can-customers-offer-claude-code-in-their-products)

由此得到的实现边界是：

- Narrant 只运行 `claude auth status` 或一次受控 probe，保存认证类别而不是 credential；
- 登录恢复动作应提示用户在外部终端运行官方 `claude auth login`，不得在 Narrant
  WebView 复制 OAuth flow；
- 不读取 `%USERPROFILE%/.claude/.credentials.json`、keychain 或任何 token；
- 若使用 API key，由最终用户在其环境/secret manager 中配置，Narrant 不提供代管或
  转售；
- executable 发现与 `claude --version` 验证应像 Codex 一样懒执行，并支持 native
  picker，但诊断不得记录完整路径。

## 与当前规格的差距

### 1. “只读且只能访问 workspace”

`--tools Read,Glob,Grep` 可以从模型上下文移除 Bash/Edit/Write/Web 等 built-in 工具，
裸 tool deny 也可移除 MCP；官方权限文档确认只读工具在 working directory 内默认无需
批准。[Permissions](https://code.claude.com/docs/en/permissions#permission-system)

但 CLI flag 不是 OS filesystem/network sandbox；继承配置、managed hook、二进制缺陷或
未来新增工具仍可能绕过“模型当前能看到的工具集合”。所以严格满足现有规格需要：

- API-key-only `--bare` + 最小 tool set，并验证 `system/init.tools`；或
- Narrant 在 Windows/macOS/Linux 分别提供外层只读 workspace 与网络隔离；或
- 修改规格，把 Claude provider 的承诺降为“模型工具级只读”，并明确它弱于 Codex。

第三项是产品语义变化，不应由实现者默认选择。

### 2. “所有分析 thread 永久删除”

官方 CLI 提供 `--no-session-persistence`，但这种 session 无法恢复；提供
`--resume`，但没有删除单个普通 session 的正式命令。`claude project purge` 会删除
整个项目的 transcripts 和其它本地状态，显然不能用于宝宝模式清理；`claude rm` 只从
background session 列表移除项目，文档明确 transcript 仍保留。
[CLI commands and session flags](https://code.claude.com/docs/en/cli-usage)

可选方案：

1. 接受 Narrant-owned Claude session 出现在本地历史，并用稳定 name/source 前缀便于
   识别；这是最小实现，但必须修改规格与隐私说明；
2. 验证 Agent SDK 是否能在 `persistSession: false` 下维持同一进程的多 turn query；
   若可行，可兼得 repair 与不落盘，需单独 spike；
3. 使用双向 stream-json 单进程配合 `--no-session-persistence`；由于官方输入协议文档
   不完整，不建议未做兼容性 fixture 就上线；
4. 不允许 repair，失败后 fresh session 重试；这会破坏共享 provider contract，不推荐。

### 3. 固定模型与成本

当前 Codex 固定 `gpt-5.6-luna`/low，但 Claude alias 会随官方最新模型漂移，完整 model
ID 又会退休。需要决定是：产品固定 `sonnet` alias 追随升级，还是维护已验证 model ID
与 fallback。JSON result 包含用量和成本估算，但现有诊断约束未允许记录成本；若要记录，
需先更新规格。[Model flag](https://code.claude.com/docs/en/cli-usage#cli-flags)

## Q2：桌面子进程的跨平台强隔离可行性

### 先修正“禁止网络”的边界定义

把 **整个 Claude Code 进程树完全断网** 与“本地 Claude 分析仍能工作”同时作为要求，
在三个桌面平台上都不可实现。Claude Code 本身负责向模型服务发请求；官方明确列出
`api.anthropic.com`、Claude 登录端点或所选 Bedrock/Vertex/Foundry 端点为运行所需网络。
[Claude Code network requirements](https://code.claude.com/docs/en/network-config#network-access-requirements)

这与当前 Codex 规格中的表述并不相同：规格允许 App Server 的 model-service transport，
只禁止 Agent tools 访问网络。因此 Q2 实际可交付的安全目标应写成：

> Claude Code 进程树只能只读访问当前 Review Workspace；不得任意访问网络，只能通过
> Narrant 控制的推理通道访问产品批准的模型/认证端点。

若没有进程外 broker/proxy，则最多只能依靠 `--tools` 移除 Web/Bash/MCP 工具，并称为
“模型工具策略”，不能称为“OS 级禁网”。Claude 官方也推荐把代理放在 Agent 安全边界
之外以注入凭据、限制 endpoint 和审计请求；标准 `HTTPS_PROXY` 的 CONNECT tunnel
不会检查 TLS 内容，严格策略需要受信任的 allowlist 或 TLS inspection。
[Secure Agent SDK deployment](https://code.claude.com/docs/en/agent-sdk/secure-deployment)
[Enterprise proxy configuration](https://code.claude.com/docs/en/network-config)

### 当前项目的平台与进程基础设施

仓库不是一个已经完成三平台发布验证的项目：

- Tauri bundle 配置写的是 `targets: "all"`，但唯一显式发布脚本是
  `build:windows`/NSIS。[Tauri config](../../src-tauri/tauri.conf.json)
  [Package scripts](../../package.json)
- CI 的 Rust/TypeScript quality gate 只在 Ubuntu 运行；真实 Tauri E2E 只覆盖 macOS 和
  Windows，且执行 debug `--no-bundle`，没有 Linux E2E、三平台 installer、签名、公证
  或商店检查。[Quality gate](../../.github/workflows/quality-gate.yml)
- 当前共享 child-process helper 在 Windows 只设置 `CREATE_NO_WINDOW`；Codex discovery
  使用 Tokio `kill_on_drop`，没有 Windows Job Object、AppContainer、Linux namespace、
  macOS App Sandbox/XPC 或统一的后代进程树 containment。
  [Child process helper](../../src-tauri/src/child_process.rs)
  [Codex discovery](../../src-tauri/src/babysit/codex/discovery.rs)

因此强隔离需要新建 platform launcher deep module，并让进程在执行 Claude **之前**进入
边界；不能先用 `tokio::process::Command` 启动，再尝试 attach policy。

### 分平台结论

| 平台 | Workspace 只读 | 禁止任意网络 | 子进程树 | 普通桌面部署 | 结论 |
| --- | --- | --- | --- | --- | --- |
| Linux | mount namespace + read-only bind 可强制 | network namespace 可 deny-all；功能模式需外部受控代理 | namespace、`no_new_privs` 与 PID 1 可覆盖后代 | 依赖 unprivileged user namespace/bubblewrap；发行版可能禁止 | 最适合先做强隔离 MVP，但必须 probe/fail-closed。 |
| Windows 11 | AppContainer/BFS 可表达指定路径只读 | AppContainer 无 network capability 可 deny-all；功能模式仍需 broker/受控 capability | AppContainer token + Job Object 可覆盖 CreateProcess 后代 | 稳定 API 需要复杂 ACL/profile launcher；新 BFS API 仍 experimental | 可做 Windows 11 实验原型，不应作为当前生产保证。 |
| macOS | App Sandbox entitlement + security-scoped bookmark 可表达，但外部 CLI 继承/传递困难 | 无 outgoing entitlement 可 deny-all；联网父 app 无法给普通 child 动态降权 | 合规签名的 child/helper 可继承 sandbox | Mac App Store 强制 App Sandbox；私有 Seatbelt 不可用 | 任意 installed Claude CLI 暂无受支持的生产方案；MVP 应禁用强保证或仅实验。 |

### Linux

Linux 的可部署路线是 `bubblewrap` 封装 user、mount、PID、IPC、UTS 和 network
namespaces：从空 mount tree 开始，只读 bind Review Workspace、Claude binary、动态
链接器及运行所需的最小系统路径，使用独立 tmpfs 作为临时写区，关闭额外 inherited
FD，并用 `--die-with-parent` 管理生命周期。bubblewrap 官方强调它只是低层 sandbox
构造工具，安全性取决于完整参数；不能 `--bind / /` 后声称已隔离。
[bubblewrap official README](https://github.com/containers/bubblewrap/blob/main/README.md)

内核能力与边界如下：

- 非特权进程先创建 user namespace 后，可在新 namespace 内取得创建 mount/network
  namespace 所需的 capability；这些 capability 不作用于宿主 namespace。
  [Linux `user_namespaces(7)`](https://man7.org/linux/man-pages/man7/user_namespaces.7.html)
- network namespace 隔离 IPv4/IPv6 stack、route 和 socket，适合 deny-all；但 deny-all
  也会阻断模型请求。功能模式需要仅连接 Narrant 外部代理的桥接设计，不能简单去掉
  `--unshare-net`。
  [Linux `network_namespaces(7)`](https://man7.org/linux/man-pages/man7/network_namespaces.7.html)
- Landlock 是非特权、可叠加且由子进程继承的 filesystem 纵深防御，但已打开 FD 不受
  约束，并且官方列出 `stat`、`chdir`、部分 metadata 和 `/proc/*/fd` 等限制；其网络
  ABI 在旧发行版不完整，不能单独承担跨发行版断网承诺。
  [Landlock userspace API](https://docs.kernel.org/userspace-api/landlock.html)
- seccomp filter 在 `no_new_privs` 后可由非特权进程安装并随 fork/exec 继承，但它只
  适合限制 syscall/address family，不能表达路径策略，预先继承的 socket FD 也需另行
  清理。[Seccomp filter](https://docs.kernel.org/userspace-api/seccomp_filter.html)

部署不是零成本：现代 bubblewrap 依赖 unprivileged user namespace。Ubuntu 24.04
默认用 AppArmor 限制其创建，应用需要安装允许 `userns,` 的 profile；不能要求用户
全局关闭安全 sysctl，因为这会扩大整机攻击面。其它发行版也必须运行时 probe，而非
只看内核版本。[Ubuntu 24.04 release notes](https://documentation.ubuntu.com/release-notes/24.04/)

**Linux MVP 推荐**：先实现 filesystem-only 强隔离 fixture；随后增加 net namespace +
进程外 allowlist proxy 才可宣称“禁止任意网络”。bwrap/userns/proxy 任一 probe 失败时
fail closed，UI 显示“此系统不能提供 Claude 强隔离”，不得静默退回普通 spawn。
`systemd-run --user` 的 `PrivateNetwork`、`ProtectSystem`、`ReadOnlyPaths` 可用于受管企业
环境，但依赖 user manager、systemd 版本与同样的 namespace 能力，不适合作为通用桌面
MVP。[systemd.exec](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html)

### Windows

Windows 的稳定底层机制是 AppContainer：它使用 package/capability SID 与对象 DACL
的交集授权，默认隔离文件、注册表、网络、credential 和其它进程；不给
`internetClient`/private-network capability 即无相应网络能力。AppContainer profile
可由普通 desktop app 按用户创建，但让 arbitrary installed Claude 读取指定 workspace
通常需要给该 AppContainer SID 增加持久 filesystem ACL，并额外暴露 executable、配置、
证书和临时目录，回滚与 symlink/reparse point 处理复杂。
[Launch an AppContainer](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer)
[CreateAppContainerProfile](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-createappcontainerprofile)

Microsoft 在 Windows 11 提供了新的 `Experimental_CreateProcessInSandbox`：其
`fs_read_only` 通过 BFS 直接表达递归只读路径，AppContainer 无 capability 时默认网络
隔离，并可添加 UI/job 限制。这非常接近需求，但截至调研日官方仍标为 experimental、
subject to change，header 不公开、需动态加载 `processmodel.dll`，最低仅写
“Windows 11 (experimental)”，而且不支持 handle inheritance。后一点会直接影响 CLI
stdio pipe，需要额外验证 named pipe/STARTUPINFO 方案。因此它适合作为技术 spike，
不适合作为 Narrant 当前 Windows 生产基线。
[Create Process in Sandbox](https://learn.microsoft.com/en-us/windows/win32/secauthz/createprocessinsandbox)

进程树还必须使用 Job Object：默认 CreateProcess children 会加入父 job；不设置
BREAKAWAY/SILENT_BREAKAWAY 时可阻止普通后代逃离，`KILL_ON_JOB_CLOSE` 可在 Narrant
退出时终止整树。但 job 只管理/限额，不提供 filesystem/network security；某些程序若
试图建立自己的不兼容 job 可能失败，必须用真实 Claude binary 验证。
[Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)

**Windows MVP 推荐**：

1. 当前稳定版只启用 Claude tool-level read-only，产品文案不得声称 OS 强隔离；
2. 单独做 Windows 11 experimental sandbox spike，验证 stdio、API-key auth、证书、模型
   proxy、Job nesting、升级兼容与卸载 profile；只允许显式实验开关；
3. 若要支持较旧 Windows 或生产强保证，开发稳定 AppContainer launcher + 临时 ACL
   grant/精确回滚 + 外部 inference broker，并进行安全审计。不能用 Windows Firewall
   的“尽力规则”代替 per-process token boundary，也不能把需要管理员安装的全局规则
   悄悄加入普通升级流程。

Windows AppContainer 本身不要求把 Narrant 发布到 Microsoft Store；但若选择 MSIX/
商店路线，package capability 与签名政策需要另行验证。当前仓库只生成 NSIS，不存在
已验证的 MSIX/AppContainer manifest 路径。

### macOS

Apple 受支持的安全边界是 App Sandbox。Mac App Store 要求 App Sandbox；站外
Developer ID 分发可选择使用，但公证要求正确签名、Hardened Runtime 和嵌套 executable
签名。嵌入 helper 的正式模式是 helper 带 `com.apple.security.app-sandbox` 与
`com.apple.security.inherit`，继承父 app 静态 entitlement。
[App Sandbox](https://developer.apple.com/documentation/security/app-sandbox)
[Embedding a helper](https://developer.apple.com/documentation/xcode/embedding-a-helper-tool-in-a-sandboxed-app)
[Notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)

这不能直接解决 Narrant 的要求：联网的 Narrant 父进程不能在 spawn 时给一个普通 child
动态去掉 network entitlement；PowerBox/用户选择文件产生的动态 sandbox extension 也不
通过普通静态继承自动传递，helper 需要接收并解析 security-scoped bookmark。任意用户
安装的 Claude CLI 不配合 bookmark，且不是 Narrant 可按 helper entitlement 重签的包内
组件。[Apple archived entitlement guide](https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/EnablingAppSandbox.html)
[Accessing files in App Sandbox](https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox)

`sandbox-exec`/`sandbox_init` 使用的 Seatbelt profile 接口已被 Apple 标为 deprecated/
unsupported，没有稳定公开 profile 契约；可用于内部原型，但不能作为公证、Mac App
Store 或生产安全承诺。[Apple Developer Forums: unsupported sandbox API](https://developer.apple.com/forums/thread/661939)

Endpoint Security 与 Network Extension/system extension 需要特殊 entitlement、Developer
ID/provisioning、用户批准或 MDM，Endpoint Security 客户端通常还涉及 Full Disk Access；
它们是企业级监控/过滤基础设施，不是轻量 per-child sandbox。引入它们会显著扩大可信
计算基，不适合宝宝模式 MVP。
[Endpoint Security](https://developer.apple.com/documentation/endpointsecurity)
[System Extensions](https://developer.apple.com/documentation/systemextensions)

**macOS MVP 推荐**：不提供“强隔离 Claude provider”，或仅在开发者开关下把私有
Seatbelt 方案明确标为 experimental/best-effort。生产探索只能是一个独立签名、无任意
网络权限、解析 workspace bookmark 的 sandboxed XPC/helper，再由受控 inference broker
承载模型请求；必须实际验证它能执行 Anthropic 发布的 Claude 安装形态及整棵后代树。
若验证失败而产品仍要求强保证，只剩 VM/远程受控环境路线；不能用普通 child spawn
冒充 App Sandbox。

### 子进程树与绕过验收

所有平台都必须在 exec Claude 前完成 containment，并覆盖 fork/exec descendants。最低
安全测试包括：

- 绝对路径、`..`、symlink/reparse point、hardlink、rename、`/proc/self/fd` 或 Windows
  handle duplicate、workspace 外读写与 workspace 内写；
- 关闭非 stdio inherited FD/handle，尤其 SSH agent、Docker socket、DBus、X11/Wayland、
  credential store、预先打开文件和 socket；
- TCP4/TCP6、UDP、DNS、Unix/abstract Unix socket、Windows named pipe、localhost 和
  inherited connected socket；
- daemonize/double-fork、child 自建 job/namespace、父进程退出、取消与 crash 后检查没有
  残留 descendant；
- 每次启动做 capability probe，并 fail closed；“policy 配置失败后普通启动”本身就是
  sandbox bypass。

### 分平台产品降级表述

| 实际能力 | 允许的用户表述 | 禁止的表述 |
| --- | --- | --- |
| 仅 `--tools Read,Glob,Grep` + deny MCP/Web/Bash | “Claude 仅获得只读分析工具；Narrant 不批准写入或联网工具。” | “操作系统保证只能读取该 workspace”“Claude 子进程已断网”。 |
| OS filesystem sandbox，但模型流量仍可直接出网 | “操作系统阻止 Claude 进程树读写 workspace 之外的项目文件；网络仍用于 Claude 服务。” | “完全隔离”“禁止网络”。 |
| OS filesystem sandbox + 只通外部 allowlist inference proxy | “操作系统限制文件访问，并将网络限制为批准的 Claude 推理/认证端点。” | “完全断网”。 |
| OS deny-all network | “此运行环境无网络访问。”同时必须禁用本地 Claude 生成 | “离线运行 Claude Code 模型”。 |
| 平台 probe 失败/只有 experimental API | “当前系统无法提供 Claude 强隔离，宝宝模式未启动（或明确选择实验模式）。” | 静默回退后继续展示“安全/只读”徽标。 |

## Q3：复用官方登录、同会话 Repair 与本地零持久化

### 约束组合的最终结论

三个目标不能在当前**公开、稳定且默认获准**的 installed CLI 集成面上同时得到：

1. 复用用户在未修改 Claude Code binary 中完成的官方 subscription 登录；
2. generation 与最多三次 repair 保持同一内存会话；
3. 不把 session transcript 持久化到本地。

技术上，TypeScript Agent SDK 的 streaming-input `query()` 加
`persistSession:false` 可以同时满足 2、3；它也可能通过底层 Claude Code credential
discovery 读到本机登录。但 Agent SDK 官方产品指南明确要求第三方 Agent SDK 产品使用
API key 或云厂商认证；除非 Anthropic 事先批准，不得向产品用户提供 Claude.ai 登录或
使用其 plan rate limits。因此“技术上能读到 credential”不构成合规授权。
[Agent SDK overview and authentication policy](https://code.claude.com/docs/en/agent-sdk/overview)
[Claude Code credential-use rules](https://code.claude.com/docs/en/legal-and-compliance#authentication-and-credential-use)

在坚持“用户自己的未修改 installed CLI + 官方登录”的前提下，最稳妥的公开接口仍是
单次 `claude -p --no-session-persistence`。要同会话多轮只能直接驱动未完整文档化的
`--input-format stream-json`，或改用 fresh-session repair。推荐后者并显式修改 spec/ADR；
若“same Agent thread”不可更改，则当前应判定 Claude provider blocked，而不是偷偷落盘
或依赖私有协议。

本机 capability probe 结果：当前开发环境未发现 `claude` executable，因此本次没有
执行登录检查、没有读取 credential，也没有触发任何模型/API 请求；所有结论来自官方
文档与官方 SDK 源码。

### 方案 A：Claude Agent SDK

#### TypeScript SDK

官方将 streaming input 定义为 persistent interactive session：一个长生命周期
`query()` 接受 `AsyncIterable<SDKUserMessage>`，自然支持顺序多轮、queued messages、
context continuity 和 interruption。`Query.interrupt()` 仅在 streaming-input 模式可用；
single-message 模式明确不支持实时 interruption 或自然多轮。
[Streaming input](https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode)
[TypeScript SDK reference](https://code.claude.com/docs/en/agent-sdk/typescript)

`persistSession:false` 的正式语义是禁用 transcript 落盘，session 只在**本次 call 的
生命周期**存在，call 结束后不能按 session ID resume。它不是“无盘但可跨进程恢复”。
`SessionStore` 也不能补上这个缺口：它镜像 session，而非替代本地 transcript，且不能与
`persistSession:false` 组合。
[SDK sessions](https://code.claude.com/docs/en/agent-sdk/sessions)
[External session storage](https://code.claude.com/docs/en/agent-sdk/session-storage)

因此宝宝模式可在一个 call 中：

1. yield generation user message；
2. 读取第一个 result，交给 Narrant 校验；
3. 无效时在**同一个仍打开的 input iterable**中 yield repair message；
4. 最多重复三次，成功后结束 iterable/关闭 Query；
5. 取消时调用 `interrupt()`，继续 drain 到对应 result 后再 close，任何 partial assistant
   message 都不得进入 Narrant 结果。

不能把 generation 做成一个 `query()`、repair 再建另一个 `query({resume})`：
`persistSession:false` 下前一个 call 结束后没有可恢复 transcript。已被移除的 TypeScript
V2 `createSession/send/stream` API 也不应采用；官方 sessions 文档记录它在 SDK 0.3.142
移除。

#### Python SDK

Python 的 `ClaudeSDKClient` 是正式的双向、有状态、多轮 client；同一个 async context 中
每次 `client.query()` 自动延续上下文，并提供 `interrupt()`。每轮必须把
`receive_response()` drain 到 `ResultMessage` 边界，再发送 repair。
[Python SDK client source](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/client.py#L24-L58)
[Python SDK reference](https://code.claude.com/docs/en/agent-sdk/python)

Python 没有 TypeScript 同名的 `persistSession` option，但当前官方 sessions 文档允许在
SDK `env` 中设置 `CLAUDE_CODE_SKIP_PROMPT_HISTORY` 抑制 transcript 写入。该组合应做一个
不调用模型的 filesystem fixture，再做显式付费 smoke，确认 SDK/CLI 版本组合没有其它
session file；不能从环境变量名称推断“绝对零磁盘临时文件”。standalone Python
`query()` 是一次性/unidirectional 接口，不适合作为 repair host。
[Python standalone query source](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/query.py#L9-L38)

#### Rust/Tauri 接入与分发

Anthropic 只提供 TypeScript 和 Python Agent SDK，没有 Rust SDK。Narrant 的 Rust backend
不能直接链接官方 SDK；可行实现是另带 Node 或 Python sidecar，由 Rust 通过 Narrant
自有、版本化的 stdio IPC 控制。这个 Rust↔sidecar envelope 是 Narrant 自己的协议，
不是 Anthropic 保证的接口。

- TypeScript SDK quickstart 说明 package 通过 per-platform optional dependency 捆绑
  native Claude Code binary；也支持显式 executable path。Narrant 若目标是“installed
  provider”，必须决定使用 bundled binary 还是用户 installed binary，不能同时把两者
  的版本/登录行为视为相同。
  [Agent SDK quickstart](https://code.claude.com/docs/en/agent-sdk/quickstart)
- Node sidecar 会给现有 Rust/Tauri/纯 WebView 架构新增 Node runtime、SDK native package、
  三平台打包签名和独立升级矩阵；Python sidecar则新增 Python 3.10+ runtime/wheel。当前
  仓库没有任何一种 sidecar 基础设施。
- TypeScript SDK 仓库声明其使用受 Anthropic Commercial Terms 约束；Python SDK 源码
  当前使用 MIT license，但服务认证/使用仍受 Anthropic 条款约束，开源许可不能覆盖
  subscription credential 政策。
  [TypeScript SDK repository](https://github.com/anthropics/claude-agent-sdk-typescript)
  [Python SDK license](https://github.com/anthropics/claude-agent-sdk-python/blob/main/LICENSE)

**方案 A 判断**：如果 Anthropic 书面批准 Narrant 复用最终用户 subscription 登录，
TypeScript streaming input + `persistSession:false` 是最清楚的同会话零持久化实现，Node
sidecar 是相对直接的 Tauri 接入；否则方案 A 只能使用最终用户自己的 API key/云凭据，
不满足本题登录前提。

### 方案 B：原始 CLI 双向 `stream-json`

CLI reference 正式列出以下 flag：

- `--input-format stream-json`；
- `--output-format stream-json`；
- `--replay-user-messages`，要求 input/output 都是 stream-json；
- `--no-session-persistence`，使 session 不落盘且不可恢复。

[CLI reference](https://code.claude.com/docs/en/cli-usage)

但是官方 CLI 文档没有完整发布 raw stdin/control protocol：没有把所有输入 envelope、
每轮 result 与下轮发送时序、interrupt control response、permission request/response、
EOF/close 语义和 forward-compatibility 规则定义为稳定 CLI 契约。官方完整示例位于 Agent
SDK streaming-input 文档，SDK 本身负责这些细节；这不等于承诺第三方可复制 SDK 内部
transport。[Streaming input](https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode)

从正式能力可以合理判断，一个不关闭 stdin 的 CLI 进程有机会在
`--no-session-persistence` 下维持内存上下文并处理多个 user envelope；但这仍需依赖
官方 SDK 源码推导未文档字段，属于 compatibility adapter，而不是稳定集成面。尤其：

- `--json-schema` 的 programmatic guide 主要描述单次 `--output-format json`，不能假定
  每个 raw streaming turn 都有相同 `structured_output` 语义；
- CLI 的 SIGTERM 会退出 143、留下未完成 turn 且无 result；零持久化后也无法恢复。
  SIGINT 与 SDK control `interrupt()` 的同进程后续可用性不能仅凭 signal 文档推断；
- 关闭/崩溃会丢掉全部 repair context，这是可接受的失败结果，但 cleanup guard 必须 kill
  整树并丢弃 partial output；
- 每个支持版本都要用 fake/no-model fixture 验证 envelope、result boundary、interrupt 后
  再发消息、EOF 和未知事件；能力 probe 失败必须 fail closed。

[CLI stop semantics](https://code.claude.com/docs/en/headless#stop-a-run-with-sigterm)

**方案 B 判断**：它最贴合“运行用户未修改 CLI、复用官方登录”，也无需 Node/Python
sidecar，但协议完整度不足，不推荐作为首版生产安全边界。若产品接受 pin/fixture 驱动的
compatibility adapter，可作为显式 experimental 路线；不能把 SDK 内部 wire schema
复制进 Rust 后称为 Anthropic 稳定协议。

### 方案 C：Fresh-session Repair

当前 Rust trait 的 `repair` 已显式传入：完整 `BabysitProviderRequest`、`previous` 完整
输出与 `ValidationFailure`。因此 Claude adapter 不依赖旧会话也能构造自包含 repair
prompt：再次提供完整 before/after/targets、上一轮 JSON 和精确验证失败；每轮均使用
`claude -p --no-session-persistence`，进程结束即无 session history。
[Provider contract](../../src-tauri/src/babysit/mod.rs#L374)

对 Review Reader 可观察行为，它仍可以保持：

- 一次完整 File Change 为分析单位，不拆逐行请求；
- `generating → repairing(attempt) → completed/failed` 进度；
- 最多三次 repair、同一十分钟总期限、严格 validation；
- 只展示最终有效结果，不缓存 partial/stale output；
- provider/cache identity 与取消行为不变。

但它**不符合当前内部契约文字**：trait 注释和 spec 都要求 repair 在同一 provider/Agent
thread；fresh session 还会重复 workspace exploration，增加 token、延迟，并可能因缺失
模型隐式上下文降低修复成功率。因此不能不改文档就宣称完全兼容。

[Babysit analysis contract](../specs/0012-babysit-mode.md#analysis-contract)

若产品接受“same thread”不是用户可观察保证，建议先更新 spec/ADR/trait comment：把
repair contract 改为“provider 必须利用完整 previous output 与 validation failure 修复；
允许 stateful same-session 或 stateless fresh-session 实现，外部进度、次数、期限与结果
校验相同”。现有 trait 参数正好支持这一定义，Codex 仍可继续复用 thread。

**方案 C 判断**：在“不代管 credential、复用用户 installed CLI、零 transcript、只依赖
公开稳定 CLI”四个条件下，这是唯一推荐的 production MVP，但前提是产品明确批准内部
same-thread 约束变更。若不批准，则没有满足全部条件的可靠方案。

### 三方案决策矩阵

| 方案 | 同一内存会话 | 零本地 transcript | 复用 installed CLI 登录 | 公开协议成熟度 | Tauri 成本 | 推荐 |
| --- | --- | --- | --- | --- | --- | --- |
| A1 TS SDK streaming + `persistSession:false` | 是，限一个 call | 官方明确是 | 技术可能；产品默认不获准，需 Anthropic 批准 | 高 | Node sidecar + SDK/binary 三平台分发 | 获批或改 API key 后首选 |
| A2 Python `ClaudeSDKClient` + skip-history env | 是，限 client 生命周期 | 文档支持抑制 history，仍需 fixture 验证临时文件 | 同上，需批准 | 高 | Python runtime/wheel sidecar | 次选 |
| B raw CLI bidirectional stream-json | 预期是，但 raw 协议不完整 | flag 正式支持 | 是，运行用户 binary | 中低 | Rust 自研 transport/compat fixtures | 仅 experimental |
| C fresh-session repair | 否 | 是 | 是 | 高，只有 one-shot CLI | 最低 | 推荐 production MVP；先修改 same-thread 决策 |

### 推荐决策

1. **默认路线**：采用 C，每轮 `--no-session-persistence`，将完整 previous/failure 放入
   repair prompt；先更新 spec/ADR 明确 stateless repair 仍满足 provider observable
   contract。
2. **若 same-session 是硬要求**：暂停实现并向 Anthropic确认 Agent SDK subscription
   集成授权；获批后采用 A1，而不是复制 raw protocol。
3. **B 只做 spike**：用 pinned CLI + fake transport fixture 验证，不把它作为首版承诺；
   capability probe 成功也不等于未来兼容。
4. **禁止的折中**：不能先持久化 session、repair 完再直接删除 `~/.claude/projects` 中
   猜测出的 JSONL。该目录属于用户，CLI 没有单 session delete 契约，race/symlink/格式
   变化会造成误删风险。

## 建议实施顺序

1. **决策先行**：确认认证档、隔离强度、session 历史政策、默认模型与 provider UI
   选择方式，并更新 spec/ADR。
2. **抽象 runtime seam**：把 Codex-specific readiness/discovery/picker/error recovery 从
   `BabysitApplication` 提升成 installed-provider runtime，不改分析 contract。
3. **做 CLI fixture spike**：覆盖 Windows argv/stdin、10 MB 边界、JSON Schema、auth
   failure、SIGINT/SIGTERM、resume repair、未知字段与进程树清理；同时验证所选 Claude
   Code 最低能力，而不是只比较版本号。
4. **实现 Claude adapter**：每轮进程 + 精确 session ID + Narrant 严格校验；复用现有
   provider contract suite。
5. **再接产品层**：provider selection、Claude-specific 恢复动作、脱敏 diagnostics 与
   smoke test；默认 CI 只跑 fake CLI，不需要安装、登录或真实付费请求。
6. **可选 SDK spike**：仅当需要不落盘 repair、typed interruption 或 Narrant 自有权限
   UI 时评估 Agent SDK；不要同时维护两套生产协议。

## 待决策项

1. Claude provider 是否只支持用户自有 API key/云凭据，以换取 `--bare` 的确定性；还是
   必须复用用户 Claude subscription OAuth？
2. “只读 workspace + 禁网”是强安全边界还是模型工具策略？若是前者，是否接受为
   Claude provider 增加跨平台 OS sandbox 的实现成本？
3. 是否允许宝宝模式 Claude session 留在 Claude Code 本地历史？若不允许，是否先做
   Agent SDK/单进程 non-persistent 多 turn spike？
4. Provider 是全局设置、每 Review 设置，还是每次宝宝模式入口选择？这影响缓存 identity、
   executable picker 与恢复 UI。
5. Claude 默认模型使用移动 alias 还是固定 ID；是否暴露模型/成本给用户？
6. 最低支持 Claude Code 版本按哪个 capability gate 确定？至少 JSON Schema 的可靠错误
   行为需要 v2.1.205；若采用 safe mode、auth status JSON 或特定 signal 行为，还需 fixture
   实测后形成能力矩阵。

## 一手来源清单

### 仓库

- [Babysit Mode spec](../specs/0012-babysit-mode.md)
- [ADR-0021: separate providers](../adr/0021-separate-babysit-agent-providers-behind-one-contract.md)
- [Provider contract and application orchestration](../../src-tauri/src/babysit/mod.rs)
- [Current Codex adapter](../../src-tauri/src/babysit/codex.rs)
- [Tauri Babysit commands](../../src-tauri/src/tauri_app/commands/babysit.rs)
- [Frontend Babysit contract](../../src/babysit/types.ts)

### Anthropic

- [Run Claude Code programmatically](https://code.claude.com/docs/en/headless)
- [CLI reference](https://code.claude.com/docs/en/cli-usage)
- [Configure permissions](https://code.claude.com/docs/en/permissions)
- [Authentication](https://code.claude.com/docs/en/authentication)
- [Legal and compliance](https://code.claude.com/docs/en/legal-and-compliance)

官方仓库中还有 headless/stream-json 的真实缺陷报告，可作为 fixture 风险样本，但不把
issue 当成稳定协议依据：
[result 后进程不退出](https://github.com/anthropics/claude-code/issues/25629)、
[成功退出但 result 不完整](https://github.com/anthropics/claude-code/issues/40432)。

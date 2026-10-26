---
status: accepted
---

# 独立 CodeGraph 项目与 Narrant 接入

关联 [Issue #205](https://github.com/XLCYun/narrant/issues/205)。总目标于 2026-10-19 确认；独立分析器和调用方调度于 2026-10-21 确认；各语言直接实现公共协议、一次输入输出及逐语言打包方案于 2026-10-22 完整确认。CodeGraph 对应规范为 `docs/specs/0001-standalone-language-analyzers.md`。

## 目标与职责

[XLCYun/CodeGraph](https://github.com/XLCYun/CodeGraph) 是独立、公开的多语言源码分析项目，其他工具可通过统一 stdio JSON 获取 Code Graph。Narrant 是首个消费者；本次包括独立项目可用、首个可下载版本和实际接入。

| CodeGraph | Narrant |
| --- | --- |
| 七套语言分析器直接实现公共协议、SDK 发现、源码分析与构图 | Git 比较、两侧固定源码准备、Code Graph Difference |
| 必要输入、来源与图不变量检查，避免重复防御与自动修图 | 安全解码、按语言调度、多语言合图和界面 |
| 固定入口、随包运行时、共同契约和语言验收 | Review、Snapshot、Review Workspace 生命周期 |
| 独立构建、不可变版本、发布清单与许可 | 固定版本下载、校验、安装、使用租约，以及调用进程树取消与超时回收 |

只迁图生成。移除跨语言 Rust 协议宿主、私有转换及分析 worker；每个语言程序自身输出最终公共图。同语言适当共享代码不形成另一套消费者接口。Narrant 的共享进程管理仍服务 Git 等功能，不能随旧分析 Host 删除。

## 接口与行为

Narrant 从已验证包启动固定 `bin/codegraph-<id>[.exe] analyze`。它不拼 Node／Java／dotnet 命令，不接受总 CLI 或私有引擎参数。

每进程只分析一次：写入一个 JSON 请求并关闭 stdin，读取一个最终 JSON 后等待退出。移除 ready/cancel；协议、引擎版本和语言身份从最终响应核对。正常 stdin EOF 不是取消。用户取消、超时或传输失败时，Narrant 终止整棵进程树，完成管道回收后才释放使用租约。启动失败、崩溃或强制终止可以没有 JSON，不能仅凭 EOF 或退出当作成功。

输入包含单项目绝对根、可选相对文件选集和既定选项。省略 `files` 按语言配置发现；`[]` 返回空图且不发现 SDK；显式非空列表只为选集构图，其他源码用于上下文及图外符号。多个独立根分别调用；Narrant 继续提供精确选集并合并混合语言图。

保留六类对象、身份、自身内容、包含、引用、扩展、测试开关及 Rust 三档依赖政策。正常 unresolved 不等于图损坏；真正的协议、输入、配置、SDK、读取或分析失败须明确报错，不伪造空图或自动修图。消费者仍检查返回图是否能安全使用。

## 能力与分发

保留 Rust、TS/JS、Python、Java、Go、C#、C/C++，支持 macOS ARM64、Windows x64。Rust、Python（Ruff/ty 的 Rust 实现）、Go 和 C/C++ 是原生入口；C# 使用 .NET apphost 与随包运行时；TS/JS、Java 使用只负责启动随包 Node/JVM 的薄入口。薄入口不解析分析 JSON、不发现 SDK、不转换图、不新增分析 worker。

沿用 [0024](0024-native-analyzer-helper-protocol.md)、[0024a](0024a-native-language-analysis-profiles.md)、[0025](0025-migrate-existing-analyzers-to-native-helpers.md) 中的语言语义、来源、只读及 SDK 政策；其旧私有消息与监督进程方式由本规范和 ADR-0039 替代。既有语言能力和验收不得因换接口而删除。

CodeGraph 独立发布固定版本分析器包，含入口、运行依赖、许可、文件清单和完整性信息，不要求单物理文件。项目目标 SDK 与分析器私有运行时分开，不暗中安装或升级目标工具链。调用方管理与调度各包，分析器不安装其他分析器。

Narrant 精确固定经过验证的版本、来源与哈希，不读取 latest 或浮动分支。开发候选不得伪装已发布；最终生产配置必须指向真实可下载且经双平台消费验收的产物。

[ADR-0039](../adr/0039-implement-codegraph-protocol-in-native-analyzers.md) 收紧 ADR-0038 的内部实现边界；此前独立分发决定继续有效。Narrant 应用仍从 XLCYun/narrant 发布，保留 `com.cyunlab.narrant`；本次不发 Narrant 应用版本。

## 验收

- 独立 CodeGraph checkout 可构建、运行和打包七分析器，无 Narrant 源码依赖。
- 每语言验证目录发现、显式选集、空集、图外引用、语义、只读、来源及 SDK 政策。
- 七入口符合公共一次性 JSON、最终身份与明确失败语义，无旧宿主、私有转换或分析 worker；双平台实际验证新入口、运行时与整树取消回收。
- 首个 CodeGraph 固定版本经人工合并及确切 main 完整 CI 后发布，真实字节、清单、哈希、内容和身份通过验证。
- Narrant 完成固定版本安装、两侧图、既有全部差异映射、混合合图和 Review；保留租约及取消回收保障。
- 相关格式、类型、Lint、脚本与受影响测试通过。Narrant PR 最新提交自动 `Complete CI` 成功，包含双平台及真实 CodeGraph 消费；不手动触发 quality-gate。
- 两仓必要 PR 人工审核合并、发布与必要收尾后，才记 #205 完成。

不增加 HTTP、LSP、常驻索引或图缓存、新语言、新平台、跨语言 FFI、公共图差异、新图展示产品；不新增仓库、Issue 或用户聊天。

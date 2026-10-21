---
status: accepted
---

# 独立 CodeGraph 项目与 Narrant 接入

关联 [Issue #205](https://github.com/XLCYun/narrant/issues/205)。本规范记录
2026-10-19 确认的总目标，以及 2026-10-21 确认的独立分析器、调用方调度和可选选集修订。
CodeGraph 的对应规范为 `docs/specs/0001-standalone-language-analyzers.md`。

## 目标

[XLCYun/CodeGraph](https://github.com/XLCYun/CodeGraph) 是公开、独立的多语言源码分析项目，
各语言分析器通过统一 stdio JSON 接口生成 Code Graph。其他本地工具可以直接使用它，无需安装或构建
Narrant。Narrant 是首个消费者。本次交付包括独立项目可用、首个可下载版本和 Narrant 实际接入。

`CodeGraph` 是项目名称；**Code Graph** 是固定源码状态下、选定文件范围内的代码对象及关系。
Narrant 将它用于 Change Comparison 的两侧，通用定义见 [GLOSSARY](../../GLOSSARY.md)。

## 职责

| CodeGraph | Narrant |
| --- | --- |
| 七套独立语言分析器、源码对象及关系投影 | Git 比较与两侧源码准备 |
| 分析 Host、输入与来源验证、完整构图 | Code Graph Difference 的计算 |
| 分析器运行环境、目标 SDK 发现、超时与取消回收 | Review Workspace、Review、Snapshot 生命周期 |
| 各分析器公开入口、共同 JSON 契约、图生成验收 | 按语言调用、返回图校验、多语言合图和界面 |
| 独立构建、版本、分析器包及发布 | 固定兼容版本的下载、完整性校验、安装和使用租约 |

只迁图生成，差异算法继续留在 Narrant。通用接口不要求 Git、Review、Pull Request 或
Narrant 数据目录存在；输入是单个固定源码目录、可选的项目内相对文件选集及分析选项。
分析期间保持源码状态一致是消费者的责任，CodeGraph 继续检查所需输入和各语言静态配置。

## 公共接口

消费者按语言直接启动相应分析器，通过 stdin/stdout JSON 交换请求、结果及取消信息；
stderr 专用于诊断。每个分析器自行定位包内依赖，完成目标 SDK 发现、输入验证、构图和回收。
消费者不需要总 CLI、不依赖私有引擎协议，也不拼接 Node／Java／dotnet 命令。
分析器可复用私有实现；内部 Rust 模块不是第二套公开接口。

省略 `files` 时，分析器按语言项目配置发现源码；显式 `files: []` 返回空图。
显式非空列表只为所选文件构图，选集外源码可用于语义解析并成为图外符号，不递归扩大结果。
发现必须遵守项目配置和既有只读加载政策。多个独立项目根由消费者分别调用；
Narrant 继续传入准备好的精确文件选集，并负责混合语言调度与合图。

每个进程处理一次分析；协议和引擎版本必须可核对。输入、输出、诊断和生命周期均有上限。
完整的来源验证、图验证和子进程回收完成后才能报告成功。协议错误、读取失败、分析失败、
超时与取消不能伪装成成功空图；省略选集与显式空集必须有不同且可验证的行为。
协议文档和示例随 CodeGraph 源码发布，并作为 Narrant 消费端的兼容依据。

本次使用本机 stdio，不增加 HTTP 服务、持久化索引、增量编辑会话或图缓存。
LSP 同样采用 JSON 消息，但其 [基础协议](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.17/specification.md#base-protocol)
不是 HTTP；使用 stdio 不等于采用 LSP 的语言服务接口。

## 能力基线

保留 Rust、TypeScript/JavaScript、Python、Java、Go、C#、C/C++ 七套分析器，支持
macOS ARM64 和 Windows x64。现有图身份、六种类别、自身内容、包含关系、引用与图外符号
语义保持不变；测试代码开关和 Rust 三档依赖政策继续有效。

[0024](0024-native-analyzer-helper-protocol.md)、[0024a](0024a-native-language-analysis-profiles.md)
与 [0025](0025-migrate-existing-analyzers-to-native-helpers.md) 是提取语义、来源、只读加载、
取消回收和各语言政策的迁移基线。CodeGraph 源码采用 `analyzers/`，语言分析器作为主要产品；
包内引擎协议属于私有实现，不能要求消费者依赖它。独立项目保留必要测试与第三方许可声明，
不依赖 Narrant 源码路径。Narrant 的共享进程管理仍服务 Git 等功能，不能随分析器一并删除。

## 版本与分发

CodeGraph 在自己的仓库发布不可变的固定版本分析器包，包含可直接启动的入口、平台运行依赖、
许可、文件清单和完整性信息；不要求每包只有一个物理文件。首个可下载版本属于本次范围。
语言包仍可按需安装；没有必经的总 CLI，分析器不安装其他分析器。
项目目标 SDK 与分析器私有运行时继续分开，不暗中安装或升级项目工具链。

Narrant 固定使用经过验证的精确版本，不根据 `latest`、浮动分支或远端未验证配置选择工具。
下载与安装继续采用已验证的归档边界、哈希校验和使用租约。开发候选不得伪装成已发布版本；
最终生产配置必须指向实际可下载且通过双平台验收的 CodeGraph 产物。

[ADR-0038](../adr/0038-call-independent-language-analyzers.md) 替代 ADR-0037 的总 CLI 分层；
CodeGraph 独立发布的决定继续替代 ADR-0036 的语言包分发归属。
Narrant 应用仍由 `XLCYun/narrant` 发布，保留 `com.cyunlab.narrant`；本次不发布 Narrant 应用版本。

## 验收

- 独立 CodeGraph checkout 能构建、测试和运行图生成，无 Narrant 源码或安装依赖。
- 七个分析器公开入口对有效输入生成符合既有图语义的结果；错误与取消明确、输出有界、进程树完整回收。
- 每种语言分别验证目录发现、显式选集、显式空集和选集外引用；消费者不需要私有参数或运行时命令。
- 七套语言的相关语义、来源、只读加载与生命周期验收在两个支持平台通过。
- 首个固定版本产物实际可下载，清单、内容、版本及协议可验证。
- Narrant 从 CodeGraph 安装固定兼容版本，按语言直接调用分析器生成两侧图，继续执行既有差异和 Review 流程。
- 相关格式、类型、Lint、脚本及受影响测试通过；Narrant PR 最新提交的 `Complete CI` 通过，
  包含双平台和真实 CodeGraph 消费路径，不能通过删除验收来完成迁移。
- 两个仓库的必要源码均经 PR 审核合并，发布及必要收尾完成后才记为 #205 完成。

本次不增加新语言、新平台、图差异公共接口或新图展示产品。使用 #205 跟踪跨仓交付；
必要的新 Issue 仍须展示具体内容并另行取得批准。

---
status: accepted
---

# 独立 CodeGraph 项目与 Narrant 接入

关联 [Issue #205](https://github.com/XLCYun/narrant/issues/205)。本规范记录
2026-10-19 确认的项目目标、职责、分发方式和交付标准。

## 目标

[XLCYun/CodeGraph](https://github.com/XLCYun/CodeGraph) 是公开、独立的多语言源码分析项目，
通过统一 CLI/JSON 接口生成 Code Graph。其他本地工具可以直接使用它，无需安装或构建
Narrant。Narrant 是首个消费者。本次交付包括独立项目可用、首个可下载版本和 Narrant 实际接入。

`CodeGraph` 是项目名称；**Code Graph** 是固定源码状态下、选定文件范围内的代码对象及关系。
Narrant 将它用于 Change Comparison 的两侧，通用定义见 [GLOSSARY](../../GLOSSARY.md)。

## 职责

| CodeGraph | Narrant |
| --- | --- |
| 七套语言 helper、源码对象及关系投影 | Git 比较与两侧源码准备 |
| 分析 Host、输入与来源验证、完整构图 | Code Graph Difference 的计算 |
| helper 运行环境、目标 SDK 发现、超时与取消回收 | Review Workspace、Review、Snapshot 生命周期 |
| 公共 CLI、JSON 契约、图生成验收 | CLI 消费适配、返回图校验和界面 |
| 独立构建、版本、语言包及发布 | 固定兼容版本的下载、完整性校验、安装和使用租约 |

只迁图生成，差异算法继续留在 Narrant。通用接口不要求 Git、Review、Pull Request 或
Narrant 数据目录存在；输入是固定源码目录、仓库内相对文件选集及分析选项。
分析期间保持源码状态一致是消费者的责任，CodeGraph 继续检查所需输入和各语言静态配置。

## 公共接口

所有消费者，包括 Narrant，启动同一个 CLI，通过 stdin/stdout JSON 交换请求、结果及
取消信息；stderr 专用于诊断。CLI 隐藏语言 helper 的启动、校验、运行环境装配和回收。
消费者无需实现各语言 helper 协议或探测编译器 SDK。内部 Rust 模块不是第二套公开接口。

每个进程处理一次分析；协议和引擎版本必须可核对。输入、输出、诊断和生命周期均有上限。
完整的来源验证、图验证和子进程回收完成后才能报告成功。协议错误、读取失败、分析失败、
超时与取消不能伪装成成功空图；无文件选集与分析得到空图应按既有规则明确处理。
协议文档和示例随 CodeGraph 源码发布，并作为 Narrant 消费端的兼容依据。

本次使用本机 stdio，不增加 HTTP 服务、持久化索引、增量编辑会话或图缓存。
LSP 同样采用 JSON 消息，但其 [基础协议](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.17/specification.md#base-protocol)
不是 HTTP；使用 stdio 不等于采用 LSP 的语言服务接口。

## 能力基线

保留 Rust、TypeScript/JavaScript、Python、Java、Go、C#、C/C++ 七套 helper，支持
macOS ARM64 和 Windows x64。现有图身份、六种类别、自身内容、包含关系、引用与图外符号
语义保持不变；测试代码开关和 Rust 三档依赖政策继续有效。

[0024](0024-native-analyzer-helper-protocol.md)、[0024a](0024a-native-language-analysis-profiles.md)
与 [0025](0025-migrate-existing-analyzers-to-native-helpers.md) 是提取语义、来源、只读加载、
取消回收和各语言政策的迁移基线。旧 helper 协议可以作为 CodeGraph 私有实现继续使用，
不能要求通用消费者依赖它。独立项目保留必要测试与第三方许可声明，不依赖 Narrant 源码路径。
Narrant 的共享进程管理仍服务 Git 等功能，不能随 helper 一并删除。

## 版本与分发

CodeGraph 在自己的仓库发布不可变的固定版本 CLI 和语言分析包，包含平台所需运行依赖、
许可、文件清单和完整性信息。首个可下载版本属于本次范围。语言包仍可按需安装，
项目目标 SDK 与 helper 私有运行时继续分开，不暗中安装或升级项目工具链。

Narrant 固定使用经过验证的精确版本，不根据 `latest`、浮动分支或远端未验证配置选择工具。
下载与安装继续采用已验证的归档边界、哈希校验和使用租约。开发候选不得伪装成已发布版本；
最终生产配置必须指向实际可下载且通过双平台验收的 CodeGraph 产物。

[ADR-0037](../adr/0037-extract-codegraph-behind-one-cli.md) 替代 ADR-0036 的语言包分发归属。
Narrant 应用仍由 `XLCYun/narrant` 发布，保留 `com.cyunlab.narrant`；本次不发布 Narrant 应用版本。

## 验收

- 独立 CodeGraph checkout 能构建、测试和运行图生成，无 Narrant 源码或安装依赖。
- 公共 CLI 对有效输入生成符合既有图语义的结果；错误与取消明确、输出有界、进程树完整回收。
- 七套语言的相关语义、来源、只读加载与生命周期验收在两个支持平台通过。
- 首个固定版本产物实际可下载，清单、内容、版本及协议可验证。
- Narrant 从 CodeGraph 安装固定兼容版本，通过公共 CLI 生成两侧图，继续执行既有差异和 Review 流程。
- 相关格式、类型、Lint、脚本及受影响测试通过；Narrant PR 最新提交的 `Complete CI` 通过，
  包含双平台和真实 CodeGraph 消费路径，不能通过删除验收来完成迁移。
- 两个仓库的必要源码均经 PR 审核合并，发布及必要收尾完成后才记为 #205 完成。

本次不增加新语言、新平台、图差异公共接口或新图展示产品。使用 #205 跟踪跨仓交付；
必要的新 Issue 仍须展示具体内容并另行取得批准。

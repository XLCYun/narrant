---
status: accepted
---

# 通过统一 CLI 使用独立 CodeGraph

多语言图生成需要供 Narrant 之外的本地工具复用，因此将 helper、分析 Host、目标 SDK
探测和构图验证迁入公开的 `XLCYun/CodeGraph`。所有消费者通过 CLI/stdio JSON 使用同一
接口；Narrant 也不直接链接其 Rust 实现。这增加了协议适配和版本兼容的成本，但把语言
工具链、进程生命周期与图生成规则集中在一个项目中，其他语言的消费者不必重新实现它们。

CodeGraph 独立发布固定版本 CLI 和可选语言包，Narrant 下载并管理它验证过的精确版本。
这替代 [ADR-0036](0036-use-one-public-github-repository-for-distribution.md) 关于语言包
归属 `XLCYun/narrant` 的部分；Narrant 应用发布、身份与升级渠道继续遵循原决策。

Code Graph Difference、Git 两侧准备和 Review 生命周期仍由 Narrant 负责。此次抽取仅涉及
图生成；本机调用采用 stdio，不引入 HTTP 服务。范围与验收见 [规范 0027](../specs/0027-independent-codegraph-project.md)。

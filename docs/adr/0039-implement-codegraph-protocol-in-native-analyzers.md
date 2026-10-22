---
status: accepted
---

# 由各语言分析器程序直接实现公共 CodeGraph 协议

2026-10-22，用户通过 Q16 明确接受完整重构方案。此前共享 Rust 入口通过 worker 调用私有语言引擎，语言语义、SDK 装配与图转换分散在多层；新结构把公共 JSON、SDK 发现、构图及必要校验集中到对应语言程序，删除跨语言 Rust 协议宿主、私有协议转换和分析 worker。

代价是各语言分别实现统一行为，需要共同契约和真实验收维持一致；同语言的模型与适当实现仍可共享。图由分析器保证有效，不增加独立复核进程、自动修图或重复防御；正常 unresolved 是合法结果。

每次启动只处理一次 JSON 输入和一个最终 JSON，随后退出；移除 ready/cancel。调用方写完关闭 stdin，EOF 是输入结束。取消与超时由调用方终止整棵进程树；崩溃或强制终止可以没有 JSON。分析器管理自己实际启动的工具，无需为取消增加监督 worker。

Rust、Python（Ruff/ty 的 Rust 实现）、Go、C/C++ 使用原生入口；C# 采用 SDK apphost 和随包 .NET；TS/JS、Java 使用仅定位并启动随包 Node/JVM 的薄入口。薄入口继承标准句柄、传播退出码，不解析 JSON、不发现 SDK、不转换图。macOS 优先 exec，Windows spawn/wait；不承诺所有分析只有一个 PID。

选择薄入口是为承接 TS ESM/Worker 与 Java JVM 目录，不增加 SEA 或 jpackage 适配；其官方能力不代表现有分析器已验证可用。C# apphost 和全部新入口必须经过双平台实际验收。

七语言双平台、选集与图语义、只读和目标 SDK 政策、独立固定版本发布及 Narrant 安装调度/合图/图差异职责继续有效。旧结构的 CI 仅是历史证据，不证明本重构完成。对应记录为 CodeGraph ADR-0001；跨仓交付由 [Narrant #205](https://github.com/XLCYun/narrant/issues/205) 跟踪。

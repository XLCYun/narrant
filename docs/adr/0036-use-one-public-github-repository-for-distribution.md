---
status: accepted
---

# 统一源码、发布和升级仓库

语言支持包的分发归属已由 [ADR-0037](0037-extract-codegraph-behind-one-cli.md) 更新为
独立 CodeGraph 仓库；本决策的 Narrant 应用发布、升级与身份约束继续有效。

用户计划公开 `XLCYun/narrant`，由同一个 GitHub 仓库承载源码、应用 Release、
应用升级 manifest 和语言支持包；取消对 `cyunlab/narrant` 分发仓库及 OSS 升级源的依赖。
CI 恢复 GitHub-hosted runner，继续面向 macOS ARM64 和 Windows x64。
这减少了双仓库权限、OSS promotion 及长驻 runner 的维护，但下载可达性将取决于 GitHub。

本决策替代 ADR-0026 的双仓库发布边界及 ADR-0029 的 OSS Stable 权威；
签名验证、可信候选暂存与退出安装等产品行为仍需保留。
不能通过整批回滚 self-hosted 时期的提交实现迁移，因为其中包含已复现并修复的
下载任务身份、Windows NSIS 启动器识别和通用 E2E 问题。

README 以 `cyunlab/narrant` 的 `596f88407e69d22083a4ca9d58c20370915145bf` 为来源，
同时处理英文版、截图和相对路径，并将发布与 Issues 链接改为本仓库。
当前 README 中仍有用的开发说明应移入开发文档，不作为替换 README 的附带损失。

## 已确认的边界

- 项目采用 Apache-2.0；第三方声明不因新增根许可证而被替代。
- 完全退役本仓库的两个 self-hosted runner：注销注册、停用自动启动，并清理
  runner 专用安装、配置、工作目录、缓存、日志及本机验证物料，不保留专用残留。
  日常 Narrant 数据、个人凭据和日常开发共用工具不属于 runner 专用物料。
- 恢复在 GitHub-hosted 临时机器上用正式安装包本体验收升级，撤销为了共享桌面
  而增加的独立升级测试包构建链；保留更新器修复及升级后的项目、设置保留检查。
  同时恢复 macOS ARM64 和 Windows x64 的正式包升级验收。

- 保留 `com.cyunlab.narrant` 及既有凭据服务名，延续已有安装与用户数据。

## 迁移交付

首个 GitHub Stable 使用受保护的明确 bootstrap tag；已有旧渠道客户端需手动安装一次
修复版，随后使用新渠道。首版安装、信任配置与恢复验收必须明确标为 bootstrap，
不能冒充上一正式版到候选的升级成功。后续发布使用正常双平台升级门禁。
本次先完成实现、审查及开源检查，正式发布使用已验证的候选与维护者批准流程。

现有 `com.cyunlab.narrant` 是 Tauri 应用标识，参与 macOS 的 bundle 与签名识别，
并用于默认数据目录。Apple Developer ID 签名身份和公证凭据在发布流程中单独配置；
迁移 GitHub 仓库不要求更换应用标识或开发者账号。凭据服务名另在代码中定义，
仅修改 Tauri 标识不会同步更名。更名方案需要明确处理旧数据发现、凭据访问和升级识别；
本次保留既有标识，不涉及 Apple 账号后台的 App ID 更名。

实施和验证状态记录在 [迁移验证](../research/open-source-migration-validation.md)。
公开前核查源码历史及 Actions 日志；声明发布成功必须以实际匿名下载和验收结果为准。

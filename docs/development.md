# 开发与语言分析

构建、开发、发布和验收脚本的目录与常用入口见 [scripts 导航](../scripts/README.md)。

## 结构分析开发

应用依赖使用 `pnpm install --frozen-lockfile` 安装；Node 版本以 package.json 为准。
七种语言的图生成由各自独立的 CodeGraph 语言分析器提供，Narrant 保留公共消费适配、图差异、
安装租约与 Review 生命周期。开发和 PR 验收入口见 [CodeGraph 支持包说明](language-support-packs.md)，
逐项测试归属见 [消费验收映射](codegraph-consumer-acceptance.md)。

当前生产目录消费已公开的固定 CodeGraph `v0.1.1` 预发布版，源码为
`d31f28a7f91c2cdbd30fd43e81e370d5ccb2c95e`。生产源按需 Python 下载与真实图验收
随完整消费命令执行。本机受控候选只证明对应语言/平台的消费链路，不能替代真实 release、
七语言或 Windows 验收；合入前仍需最新提交的 `Complete CI` 通过。
原生迁移的历史结果仍保留在 docs/research；这些结果不自动成为新语言入口的消费证据。

同一比较侧各语言继续共用 Review 的 Git workspace。按需文件分组、测试文件筛选、
Rust 三档依赖政策、图合并和前端 DTO 保持不变。客户端只发送公共分析选项，运行时与
目标 SDK 发现由 CodeGraph 负责；目标项目所需工具链由用户或 CI 单独准备。
任何协议、分析、退出或清理失败都会使结构比较失败，不转换成成功空图。

## 构建 Windows 安装包

安装项目依赖后，在 Windows 系统的项目根目录运行：

```powershell
pnpm build:windows
```

该命令仅构建 NSIS `.exe` 安装包。构建产物位于：

```text
src-tauri\target\release\bundle\nsis\Narrant_<version>_x64-setup.exe
```

## 本地开发

### 合并前 CI

指向 `main` 的 PR（包括草稿）在创建、推送新提交或重新打开时自动运行 `CI`。
入口没有文件路径过滤，同一 PR 的新运行会取消尚未结束的旧运行。

- `quality-gate.yml` 运行 macOS ARM64 / Windows x64 的 Rust 静态检查、单元、集成和
  文档测试，以及双平台真实 Tauri E2E；前端 Lint、类型检查、构建、组件与脚本测试也在此完成。
- `validate-language-support-packs.yml` 下载固定 CodeGraph 七包，在双平台执行真实语言入口、
  图差异、安装与 Review 验收；helper 源码与发布由 CodeGraph 自身 CI 负责。
- `Complete CI` 只在上述两套验收都成功时通过；失败、取消、跳过或缺失结果都不能放行。
  `main` 的必过检查使用此名称，合入前还要求分支包含最新 `main`。

PR 验收使用标准 GitHub-hosted 临时机器，不需要签名或发布凭据。签名、公证、Release 和
正式安装升级验收仍由发布流程执行；PR 通过不等于正式包升级已通过。真实模型 endpoint
等人工专项仍按各自说明执行，不纳入公开 PR 的确定性测试。

日常修改继续先做本地聚焦验证，无需为每次小改重复完整测试。修改已推送后，以当前 PR
完整 CI 的结果为合并依据；此前提交或部分专项的绿灯不能替代它。

Custom Babysit 的确定性 smoke、终端开发日志和数据保密边界见
[开发日志](agents/dev-logging.md) 与 [Babysit smoke test](agents/babysit-smoke.md)。

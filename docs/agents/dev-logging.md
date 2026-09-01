# 开发日志

Narrant 的 Debug 构建会把 Rust `tracing` 事件直接打印到启动 Tauri 的终端。日志使用
`tracing-subscriber::fmt` 的单行人类可读格式，不写入文件、不通过网络导出，也不需要 Venator、
OpenTelemetry、Collector、dashboard 或 Docker。Release 构建不会安装日志 subscriber。

## 1. 默认日志

从仓库根目录运行普通开发命令即可：

```sh
pnpm dev
```

未设置 `RUST_LOG` 时，Debug 构建默认使用 `narrant=info,rig=info`。它会显示 Babysit 分析生命周期、
Custom generation/repair 进度和 Rig 模型请求等信息。关键结束事件显式包含 `elapsed_ms`，不需要从
相邻时间戳人工计算主要耗时。

## 2. 详细诊断

需要查看逐轮模型调用和 repository tool 边界时，提高 Narrant 与 Rig 的日志级别：

```sh
RUST_LOG=narrant=debug,rig=debug pnpm dev
```

也可以使用标准 `tracing-subscriber` EnvFilter 语法只提高一个模块的级别。无效的 `RUST_LOG` 不应
阻止应用启动；Narrant 会打印一条不包含环境变量原文的安全警告，并回退到默认过滤器。

一次 Custom 分析应能在同一个 `analysis_run_id` 下看到：

- 分析开始、generation/repair、完成/取消/失败与总耗时；
- Rig 每轮模型调用以及 provider HTTP 等待的开始、结束、状态和耗时；
- Read、Glob、Grep、`final_result` 的调用序号、字节计数、截断状态、结果状态和耗时；
- 输出上限、总超时、取消和稳定的 provider 错误分类。

本日志只用于定位时间花在模型服务、Rig agent loop、Narrant validation/repair 还是本地工具，不改变
十分钟分析 deadline、单请求超时或 agent loop 行为。

## 3. 数据边界

开发日志允许打印配置的完整 endpoint URL 和 model identifier，以便确认请求目标。不要把带有
userinfo、query token 或其他秘密的值放入 endpoint URL；Narrant 不负责清理该 URL 后再打印。

日志不得包含：

- API key 或其他独立 credential；
- prompt、source、模型输入/输出正文或 Line Explanations；
- repository tool 参数、匹配内容或工具结果。

Rig 请求必须继续设置 `record_content_telemetry(false)`。工具日志只记录工具名、序号、输入/输出
字节数、截断状态、结果状态和耗时。分享终端输出前仍应检查 endpoint 与 model 是否适合公开。

## 4. CI 与发布边界

CI 使用确定性 fixture，不运行真实 model endpoint，也不依赖日志文本颜色、时间戳或完整排版。
日志测试只验证过滤、关联字段、耗时字段、允许/禁止的字段集合，以及 Release 不初始化 subscriber。
不要手动触发 `quality-gate`。

真实 Custom endpoint 的人工验证步骤见 [Babysit smoke test](babysit-smoke.md)。

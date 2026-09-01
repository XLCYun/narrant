# 开发观测：Venator

Narrant 的开发观测是 opt-in 功能。默认的 `pnpm dev`、`pnpm td`、`pnpm build`、前端测试和 CI
不会启用 `dev-observability`，也不会向模型服务或 OTLP receiver 发送请求。本页只用于本机调试，
不要把它作为生产部署方案。

## 1. 安装并启动 Venator

Venator 是本地桌面日志和 trace 查看器。优先从其 [Releases](https://github.com/kmdreko/venator/releases)
下载与你的系统匹配的预构建应用（macOS Intel/Apple silicon 或 Windows x64），安装后启动 Venator。
也可以在本机 Rust 工具链可用时从源码安装：

```sh
cargo install venator-app
venator
```

启动后保持 Venator 运行。它默认监听本机 `127.0.0.1:8362` 的 OTLP receiver；Narrant 会以
OTLP HTTP/protobuf 发送 traces 和 logs，并为两个 signal 分别追加 `/v1/traces` 与 `/v1/logs`。
不需要 Docker、Collector 或额外配置。

## 2. 显式启用 Narrant 观测

在另一个终端，从仓库根目录运行：

```sh
pnpm dev:observability
```

这个命令等价于 `pnpm tauri dev --features dev-observability`，会启动真实 Tauri 应用和前端热更新。
需要验证带观测 feature 的打包构建时运行：

```sh
pnpm build:observability
```

这两个命令都通过仓库的 `pnpm tauri` wrapper 运行；普通 `pnpm dev` 或 `pnpm tauri dev` 不会启用
观测。没有 Venator 时，应用仍应启动，终端中的安全生命周期日志仍可用，分析不会因为 dashboard
缺席而阻塞。

## 3. 覆盖 OTLP endpoint

未设置或设置为空的 `OTEL_EXPORTER_OTLP_ENDPOINT` 使用 Venator 默认地址。需要临时发送到其他
OTLP/HTTP receiver 时，只通过当前进程的环境变量覆盖 base URL；不要把真实地址写入脚本、仓库、
issue、PR、截图或共享日志：

```sh
OTEL_EXPORTER_OTLP_ENDPOINT="$OTLP_BASE_URL" pnpm dev:observability
```

其中 `OTLP_BASE_URL` 应由本机未提交的环境管理方式提供，不要把值直接替换后提交这段命令。它是
receiver 的 base URL，不要预先拼接 `/v1/traces` 或 `/v1/logs`；应用会在启动时规范化结尾斜杠并
为每种 signal 追加路径。覆盖只在本次进程启动时读取，修改后需重新启动观测构建。

## 4. 数据保密边界

- 只在明确启用 `dev-observability` 时导出 telemetry；API key 只在 Custom 设置 UI 中输入，不要
  放进 shell 命令、环境变量、端点路径、仓库文件或调试参数。
- 导出的 Narrant lifecycle 字段只包含 run id、provider、protocol、phase、attempt 和稳定的
  error category。Rig 的 GenAI content telemetry 已关闭。
- telemetry、终端日志、截图和人工记录都不得包含 key、完整 endpoint、prompt、source、response、
  explanation、tool arguments 或 tool results。失败时只记录稳定错误分类和是否完成。
- 运行人工 smoke 前使用 disposable Review Workspace；不要复制、导出或分享模型输入输出。结束后
  清空表单并删除临时数据；不要提交 Debug 凭据文件或包含真实连接的本地应用数据目录。
- 不要通过 shell trace（例如 `set -x`）、命令回显、录屏或日志采集暴露连接值；确认命令或 CI
  日志没有打印环境变量。

## 5. CI 边界

Venator 和真实模型 endpoint 明确不属于 CI。`.github/workflows/quality-gate.yml` 继续使用默认
feature 的 `pnpm lint`、`pnpm build`、`pnpm test` 与确定性 Tauri E2E；不要在 workflow 中添加
`pnpm dev:observability`、`pnpm build:observability`、Venator 启动或
`test:e2e:custom-live`。真实 Custom probe 只能由 reviewer 明确授权后在本机执行，且 probe 用例
使用 `*.smoke.ts` 命名，不会匹配默认 WDIO 的 `*.e2e.ts` glob。

前端宽/窄窗口和 Custom 各状态的 Preview Harness 验收步骤见
[Babysit smoke test](babysit-smoke.md)；这些步骤同样不需要设置凭据或启动 Venator。

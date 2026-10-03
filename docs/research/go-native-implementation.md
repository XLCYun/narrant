# Go 原生 helper 实施与本机验证

日期：2026-10-03。源码在 `native-helpers/go/`。此记录覆盖 Go helper 自身；公共 Host、支持包、Windows 原生运行和完整产品验收分别由对应任务负责。固定本机实验工具为 `/usr/local/go/bin/go` 1.23.8，宿主为 macOS arm64。

## 已实现边界

- 独立 Go module，只有 Go 标准库依赖。编译后以 stdin/stdout NDJSON 执行 v1 协议，`ready` 后只处理一个请求。严格 JSON 解码拒绝重复键、未知字段、缺字段和第二种消息；运行中独立读取匹配 ID 的取消。输出完成消息后直接返回。
- 静态读取本侧 `go.mod`、`go.work`；`use` 与未带版本的本地 `replace` 必须留在仓库内。无 `go.work` 时只接受一个选中 module。只读 importer 仅允许仓内 module 与传入目标 SDK 的标准库源码；不调用 `go list`、`go/packages`、默认 importer、GOPATH、模块缓存、网络或项目构建逻辑。
- catalog 元数据固定 `kind=go-sdk`、`version=1.23.8`、`root` 指向完整 Go SDK 源码，metadata 为 `goos`、`goarch`、amd64 时 `goamd64=v1`。候选 OS 为 darwin/linux/windows，架构为 arm64/amd64，cgo 关闭。检查选中包内所有允许源码（含未选中的平台文件），只把真正的 Go 文件末尾平台后缀当作目标证据，并按 Go 的文件匹配规则逐目标求值受支持的 `//go:build` / legacy `+build` 布尔条件。每个落在固定 profile 目标矩阵中的明确上下文文件都须有 catalog 匹配；没有明确约束时只取 helper 原生 GOOS/GOARCH 基线。选中文件在所有获准目标下都不活动则失败。
- 每配置分别使用 `go/parser`、`go/ast`、`go/types`。文件、类型、函数/方法、值/字段、import、共享多名初始化分别映射为 Container、Type、Callable、Value、Extension、ExecutionRegion。`init` 为 Callable；类型参数保留在声明，跨包泛型调用指向原始声明。接口调用同时保留静态接口方法槽位和未知动态实现观察。字段/嵌入类型位置有 `types.Info` 证据时输出 `code.type_use`，未知绑定保留 Unresolved。已确认调用最终按图成员分类为 Node 或 External，缺依赖/函数值调用保留 Unresolved。SDK 方法图外身份包含 receiver 类型，避免同包同名方法混淆。
- ID 使用相对源文件、声明种类、名称与保守同名槽位，不含 checkout 绝对路径、行号或类型检查对象地址。方法 ID 只使用 receiver 类型，不使用 receiver 参数名。原始 `source` 作展示，signature/code 通过 Go token 序列规范化；普通注释与排版不改内容，字面量与必要分号保留。类型子字段/接口方法独立，文件记录初始化顺序，结构体记录字段顺序。
- 输出完整 `root_contains`、`nodes`、`external_symbols`、`evidence`；跨配置同身份自身内容及父级不兼容返回 `union_conflict`。结果大小由 helper 再核对 `max_result_bytes`，Host 仍独立限额。

## 可复验结果

在 `native-helpers/go/` 执行：

```sh
/usr/local/go/bin/go test -v ./...
/usr/local/go/bin/go vet ./...
```

2026-10-03 两命令 exit 0。初版 9 个聚焦测试通过：真实双 module `go.work`、Linux/Windows 平台互斥声明及公共调用边并集、仓内未选包和已选包、`strings.Contains` 标准库绑定、缺失第三方未知调用、函数正文/子字段/纯格式投影、重复路径和自定义标签拒绝、协议终结自行退出、严格 JSON、取消、接口与多名初始化、测试文件排除/恢复、仓外符号链接拒绝。双 module fixture 分析前后的路径和文件字节映射完全相同，且未出现 `go.work.sum`。独立审查后的 8 项重要问题及进行中取消入口问题已加 `review_test.go` 回归，结果和精确命令见 Task 2 报告。

真实 Host→CodeGraph→diff 的 Go conformance 由整体验收任务执行；其反馈发现单行/多行函数末尾隐式分号造成误报。本 helper 已将 `}` 前可选分号规范化，并添加对应回归断言。该修复后本目录测试再次通过。

## 明确不支持或尚未验证

- 首轮只验证 Go 1.23.8 SDK。`go`/`go.work` 语言版本需属 1.23 系列；其它 helper 构建版、SDK 版和语法兼容性未验证。这里的跨目标类型检查不证明已在 Linux/Windows 原生运行；Windows x64 CI 仍是发布门槛。
- 静态配置子集拒绝带版本的 `replace`、仓外 `replace/use`、多 module 但无 `go.work`、未显式 `use` 的嵌套 module、自定义 build tag、vendor、cgo、embed 和外部 `_test` package；支持已知标签的布尔及取反条件。不存在覆盖任意 Go 项目、目标矩阵或 tag 组合的承诺。Go 1.23 的标准库源码闭包只由已测试导入样本证明，不代表整套标准库都通过验证。
- 函数体可以有缺第三方导致的类型错误；只在语法树和选中声明仍完整时保留已确认调用与未知观察。当前不做函数值数据流、反射、运行时接口实现集合或调用次数推断。缺目标 SDK、缺选中文件覆盖、声明对象缺失或未知必要配置返回原子错误。
- 原位只读由实现入口审计（`os.ReadFile`、`go/build.ImportDir`、parser/types）和路径/字节夹具支持，尚未做系统级进程跟踪或通用 OS 文件系统沙箱。Host 的工作区最终检查仍须独立执行。

## 最终审查修复补充（2026-10-03）

以上为此前实施快照。本轮针对最终审查补充修复：空白包变量的独立求值有稳定
ExecutionRegion 与调用归属，多结果共享 initializer 仍只归一个区域；平台发现沿显式仓内
导入闭包读取各包条件，必要源码/配置失败不会降为缺失第三方绑定；泛型 receiver 剥离
指针、括号和类型实参后定位基础类型，成员关系不再指向伪造的类型参数 External。

新增五个行为回归先在旧源码复现失败，再在修复后通过。最终
`/usr/local/go/bin/go test -v ./...` 为 23 个顶层测试通过（另有两个取消子案例），
`go vet ./...` 和 `go build -trimpath -o /private/tmp/narrant-go-final-fix .` 均 exit 0。
覆盖单独/逐槽/共享空白初始化、参数编辑稳定身份及调用目标、仓内未知 tag 和损坏语法、
传递导入的 Linux/Windows 配置并集及缺 catalog 目标、值/指针泛型成员归属。
未参与分析的依赖 `_test.go` 在必要配置校验前排除；若测试实际启用，其未知必要条件仍失败。
这些是新源码本机验证；旧支持包不代表本轮产物，包刷新和真实 Host 公共验收另行记录。

## 最终平台结果（2026-10-03）

`4c65bb8` / [运行37111018854](https://github.com/XLCYun/narrant/actions/runs/37111018854)
已完成该helper及公共新native链路的macOS ARM64/Windows x64验收，含真实安装和全部8项公共图测试。
精确语言/SDK、计数、日志和未覆盖范围见[跨平台记录](native-crossplatform-validation.md)。
总CI仍因旧共享WindowsJob回归失败；本轮未修改其行为，未发布支持包，也不扩大前文语言支持范围。

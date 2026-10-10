# Go 原生 helper 源码加载探针

日期：2026-10-02。此目录是可丢弃的研究探针，不是产品实现或发布验收。

## 问题与结果

本次验证：能否绕过 `go list` / `go/packages` 项目导入，仅使用 `go/parser`、`go/types`、严格源码 importer，分别分析两个明确平台，再对有证据的调用观察取并集。

已在本机 Go **1.23.8 / darwin arm64** 实测通过。分析目标是 **linux/amd64** 与 **windows/amd64**，均关闭 cgo。此处跨目标类型检查不等于在 Linux 或 Windows 上运行 helper。完整观察见 [results-2026-10-02.json](results-2026-10-02.json)。

| 检查 | 结果 |
| --- | --- |
| 互斥文件选择 | Linux 只选 `platform_linux.go`，Windows 只选 `platform_windows.go`；公共文件两者均选 |
| 公共 `Run()` 的平台调用 | 分别绑定到两个不同源文件的 `platform()`；并集保留两个真实源码目标 |
| 仓库中的另一包 | `local.Local()` 解析到显式允许的仓库源文件声明 |
| 标准库 | `cmp.Compare()`、`strings.Contains()` 都有真实类型绑定；递归读取目标 SDK 标准库源码，Linux 29 个包、Windows 28 个包（不计内建 `unsafe`） |
| 第三方源码已存在 | 专用 GOPATH 中放置 `example.test/missing`，仍按来源策略拒绝；没有因为磁盘上可读而纳入 |
| 缺依赖错误 | 每个平台产生一个明确导入错误；`missing.Remote()` 保留未知，其他已确认调用仍可取得证据 |
| 并集 | 8 条去重后的观察，其中 7 条解析成功、1 条未知；这不是完整 CodeGraph |
| 原位内容 | 分析前后比较 fixture 的全部 8 个路径及文件 SHA-256，完全一致；没有新建 `go.work.sum` |

## 复跑

从仓库根目录运行：

```sh
NARRANT_GO_PROBE_BIN=/usr/local/go/bin/go python3 docs/research/probes/go-native-helper/run.py
```

脚本仅接受已有 Go 1.23 SDK；不会安装 SDK、下载依赖或运行仓库构建。它在临时目录生成自己的 fixture，用 `go env` 识别已有 SDK，用 `go build` 编译本目录的研究 helper，随后直接运行 helper。编译的是分析工具，fixture 程序始终只被解析和类型检查。所有 Go cache、GOPATH、模块缓存及临时编译输出均指向实验目录；未写入仓库分析目标。

分析阶段的 importer 只认识一个显式映射的仓库包及指定 `GOROOT/src` 内的标准库源码。它不使用 `go/importer.Default`、导出数据缓存或业务模块缓存；`unsafe` 使用 `types.Unsafe`。标准库与仓库上下文包跳过函数体，仅检查声明所需信息，根包检查函数体。标准库源码导入成功，**本样例分析阶段不需要外部工具生成标准库导出数据**。

文件选择使用 `go/build.Context.ImportDir`，指定 `build.IgnoreVendor`，并固定 `GO111MODULE=off`；已核对本机 Go 1.23.8 的 `go/build/build.go`，这个模式令 `importGo` 直接拒绝 `go list` 回退。helper 本身没有进程调用 API，分析阶段 PATH 也指向不存在的工具目录。这里的“无外部命令路径”来自实现与已安装标准库源码复核，**未做系统级进程跟踪**；空 PATH 本身不能阻止绝对路径命令，不能当成沙箱保证。

两个目标显式使用 `types.SizesFor("gc", "amd64")`，并固定 Go 1.23 默认实验标签及 `amd64.v1`，避免宿主 arm64 特征污染选择。SDK、语言版本、release tags 和实验标签此处恰好来自同一个固定 Go 1.23 系列；尚未实现一般工具链版本转换。

## 证据的边界

- fixture 中有 `go.mod` / `go.work`，但探针**没有解释它们**。仓库包路径是手工允许映射，配置也是手工指定两个平台；尚未证明模块、workspace、replace、vendor、测试变体或自动配置发现可正确加载。
- 只验证本例保留下来的类型证据。错误项目中的 `go/types.Info` 可能不完整；不能推广为所有缺依赖项目都能产生完整对象和正确关系。产品仍必须遵守对象不完整则失败的契约。
- 并集只对本例调用观察取集合；尚未构造六类对象、稳定身份、内容投影、包含关系或验证冲突。`path#name` 只是方便阅读的实验身份，不能作为生产身份算法。
- 标准库只覆盖这两个导入闭包，声明导入忽略函数体；未验证其他 SDK、完整 stdlib、目标版本兼容、cgo、汇编/内建差异、import 可见性和导入循环的所有语义。
- 前后路径和内容一致证明本次没有持久化修改，无法检测“写入后恢复”的瞬时行为；没有用权限位宣称原位只读保证，也没有实施生产文件系统隔离。
- 没有运行 `go/packages`、语言服务、Narrant、Cargo、端到端验收、发布打包、性能测试或 quality-gate。

## 下一步加载路径

这一结果让 **受限项目模型 + 源码 importer + `go/types`** 成为值得继续验证的具体路线；它消除了本例对默认 `go list` driver 的依赖，但尚不能取代完整 loader。

下一步优先定义小而明确的只读项目 profile：静态读取 `go.mod` / `go.work`，仅解析仓内 module/use/replace 与被允许的已提交源码，形成显式包映射；按已确认的目标配置选择源文件；拒绝越界和不支持的必要配置。再以多 module、vendor、测试包、缺依赖与目标 SDK 矩阵验证该 profile。若采用 `go/packages` 自定义 driver，也仍须由这层提供可信元数据，不能再自动回退默认项目导入。是否足够覆盖首版目标项目，须由这些后续实验决定。

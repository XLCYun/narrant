# CodeGraph 语言支持包消费

Narrant 从 `XLCYun/CodeGraph` 消费固定 `v0.1.0`，包与引擎版本均为 `0.1.0`，
协议为 `codegraph.analyzer/1`。当前七包保持 `published: false`；真实发布后由发布切换任务
更新该标记并运行完整双平台消费验收，全部通过才可合入。应用发布继续使用 `XLCYun/narrant` 和 `com.cyunlab.narrant`。
职责见 [规范 0027](specs/0027-independent-codegraph-project.md)。

## 公共边界

应用从已持有使用租约的包根启动 `bin/codegraph-<id>[.exe] analyze`。七个固定 ID 为
`rust`、`typescript-javascript`、`python`、`java`、`go`、`csharp` 和 `c-cpp`。
每个入口自带语言身份，消费者不传语言选择器或包根参数。
它只传源码根、相对文件选集、测试开关与 Rust 依赖范围，验证 ready 的引擎/协议/包身份，
严格构造返回图，并等待分析器、嵌套 worker/helper 和管道线程退出后返回。
Code Graph Difference、Git 两侧准备、Review/Snapshot、工作区保护与界面都留在 Narrant。

公开契约以 CodeGraph 的 [protocol](https://github.com/XLCYun/CodeGraph/blob/main/docs/protocol.md)、
[distribution](https://github.com/XLCYun/CodeGraph/blob/main/docs/distribution.md) 和
[pack layout](https://github.com/XLCYun/CodeGraph/blob/main/docs/pack-layout.md) 为准。
消费者不读取 `control/native.json`，也不装配 helper argv、运行时或 SDK catalog。
必需入口为对应语言的可执行文件和五字段 `control/codegraph.json`；私有文件按 manifest
完整性清单验证。Narrant 每次显式传入文件选集；省略 `files` 的配置发现能力属于分析器，
不改变 Narrant 的两侧文件准备、语言路由与图合并。

## 安装与隔离

固定 Release 的资产为 `<id>-<target>-manifest.json`、`<id>-<target>-pack.tar.gz` 和
`SHA256SUMS`。下载器验证清单及归档的发布摘要，manifest 再验证版本、引擎、协议、完整
`sourceCommit` 与 `sourceDirty: false`，随后有界解包并逐文件验证摘要。
归档、解包总文件和清单上限分别是 256 MiB、1 GiB、2 MiB。

命令行下载器把每包的清单、归档和 `installed/` 放在同盘临时目录，全部校验通过后整目录
提升到 `<output>/<id>/0.1.0/<target>`。七包成功后才原子替换 `candidates.json`；下载中断、
摘要失败或解包失败会清理当次临时内容，可在同一输出目录重试。重试只复用与本次发布摘要、
逐文件大小/摘要/权限及描述符均一致的完整包；旧下载残片和损坏缓存会重新获取。
缓存未通过复验时，先移除旧候选索引，避免重建失败后继续暴露无效候选。
候选 `root` 指向各包内的 `installed/`，不改变下述应用安装目录。

应用数据目录下继续使用 `language-support-packs/installed/<id>/<version>/<target>`，
暂存位于同盘 `staging`，跨进程使用/安装/删除共用原有 `id-version-target` 锁。
旧 Narrant 包或只有通用 `bin/codegraph[.exe]` 的旧候选即使具有相同 ID 和 `0.1.0`
版本，也因缺少当前公开身份或固定语言入口而不可用；
替换必须取得原锁的独占权限，不能覆盖仍被旧进程使用的目录。
错误与取消不能返回成功空图；下载失败保留明确重试状态。

## 验收入口

正式产物发布后，从仓库根执行：

```sh
pnpm codegraph:download .tools/codegraph-packs
pnpm codegraph:verify .tools/codegraph-packs
```

下载器只接受目录中的已发布固定版本。当前未发布状态会明确失败，不能用 skip、浮动 tag、
历史 Narrant release 或临时重建 helper 替代真实发布证据。Narrant 不再构建或发布语言 helper。

受控本机候选验收使用 CodeGraph 生成的真实 `manifest.json` / `pack.tar.gz`，按
`<artifacts>/<id>/0.1.0/<target>` 放置，并提供只含公开六字段的候选数组：

```json
[{"id":"typescript-javascript","version":"0.1.0","engineVersion":"0.1.0","protocol":"codegraph.analyzer/1","target":"aarch64-apple-darwin","root":"/absolute/verified/pack-root"}]
```

```sh
node scripts/codegraph/verify-consumer.mjs /absolute/artifacts --candidates /absolute/candidates.json --language typescript-javascript
```

`root` 仅用于本机测试，不能写入正式 manifest。指定语言缺少候选或真实归档时必须失败。
安装测试用本地 HTTP 提供实际候选字节，仍通过生产校验与事务安装；它不构成远端 release
可下载证据。38 项完整 diff 测试的逐项迁移和入口见 [消费验收映射](codegraph-consumer-acceptance.md)。

## 目标项目 SDK

支持包内的私有运行时由 CodeGraph 提供。被分析项目仍可能需要已有 Rust/Cargo/rust-src、
JDK、Go SDK、.NET reference packs 或系统 C/C++ headers；分析不会安装这些工具。
Narrant CI 的 `resources/codegraph-test-sdks.json` 和 `scripts/language-packs/prepare-test-sdks.mjs`
只准备保留夹具使用的目标项目 SDK，不构建 helper，也不改变应用的公共请求。
Java 使用 `JAVA_HOME` / `JDK_HOME` / PATH 中的 Java 安装，Go 使用 `GOROOT` / PATH 中的 Go；
多版本 CI 夹具将各 SDK 的 bin 放到 PATH。.NET 使用正常 `DOTNET_ROOT` reference pack 布局。
Rust 三档与测试开关继续按原语言规范执行。

## CI 与发布交接

PR 自动调用 `validate-language-support-packs.yml`，在 GitHub-hosted macOS ARM64 与
Windows x64 下载精确七包，执行独立语言入口、完整图差异、安装、Review DTO 和退出屏障验收。
CodeGraph 源码、helper、SDK 工厂与私有协议测试由其自身 CI 负责。
`Complete CI` 仍要求 quality 和 languages 两组均成功，缺失、取消或跳过均不能放行。
最终切换发布版本还需要真实远端下载和最新双平台 CI 证据；此前的本机候选结果不能替代。

# 原生 helper 真实图验收

`src-tauri/tests/native_helper_graphs.rs` 是通过公开 `NativeLanguageAnalyzer::analyze`
和生产 `CodeGraph::diff` 运行的端到端测试。默认 `cargo test` 将它们标记为 ignored，
因为测试需要已经固定的真实 helper 可执行文件和目标标准库目录。测试没有假 helper，
也不把 helper CLI JSON 当作最终验收图。

先将四个 helper 及其目标目录放在分析 fixture 仓库之外，写一个 JSON 清单。路径必须是
绝对路径；`trusted_roots` 包含可执行文件所属目录，且这些目录不能与测试创建的临时仓库
重叠。`platforms` 的每条记录直接对应公开 Host `Platform`，字段和值由支持包构建过程
产生，不在测试中探测本机默认 SDK。Java 可执行文件通常是私有 JDK 的 `java`，其 `args`
需包含固定 classpath 和主类；C# 使用私有 dotnet host 加固定 helper DLL 参数，Go/Clang 不需启动参数。

```json
{
  "go": {
    "executable": "/absolute/pack/bin/narrant-go",
    "args": [],
    "environment": {},
    "trusted_roots": ["/absolute/pack"],
    "platforms": [{
      "id": "go-darwin-arm64", "kind": "go-sdk", "version": "1.23.8",
      "root": "/absolute/pack/runtime/go", "fingerprint": "sha256:...",
      "metadata": {"goos": "darwin", "goarch": "arm64"}
    }]
  },
  "java": {"executable": "/absolute/pack/runtime/jdk/bin/java", "args": ["-cp", "/absolute/pack/runtime/helper/classes:/absolute/pack/runtime/helper/lib/*", "dev.narrant.java.Main"], "environment": {}, "trusted_roots": ["/absolute/pack"], "platforms": []},
  "csharp": {"executable": "/absolute/pack/bin/dotnet", "args": ["/absolute/pack/runtime/helper/Narrant.CSharp.Helper.dll"], "environment": {}, "trusted_roots": ["/absolute/pack"], "platforms": []},
  "clang": {"executable": "/absolute/pack/bin/narrant-clang", "args": [], "environment": {}, "trusted_roots": ["/absolute/pack"], "platforms": []}
}
```

上面的空 `platforms` 仅展示清单结构，不能通过完整验收。真实清单需为 Java 提供
`java-jdk` 和 release 17，为 C# 提供 `dotnet-reference-assemblies` 和 net10.0，
为 Clang 提供 `clang-target` 与对应的 `clang-resource`；Go 目标须与运行宿主或
fixture 约束匹配。字段须满足 Host 与各 helper 的严格验证。

完整执行：

```sh
NARRANT_NATIVE_HELPERS_CONFIG=/absolute/native-helpers-config.json \
  scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml \
  --test native_helper_graphs -- --ignored
```

一轮完整执行包含配置完整性、Go、Java、C#、Clang C17、Clang C++20 五种模式，以及同一仓库的 mixed 图合并，共七个测试。
`all_required_helpers_configured` 会使缺失任一 helper 配置的完整执行失败；
单独调试 Go 时可用 `-- --ignored go_real_helper_graph_and_diff` 聚焦运行，
这种聚焦结果不能当作完整验收。

每种 fixture 都有已选调用者、已选跨文件目标和未选的仓内目标。断言从实际
`CodeGraph` 根遍历节点，再核对 `code.calls` 的 Node 端点、Repository External
描述和目标身份，不依赖后端的私有 ID 编码。before/changed/formatted/relocated/relations/structure 六份
独立仓库验证子对象正文变化、普通注释与排版不变更、迁移绝对路径后的稳定身份，
关系改绑变体将同一调用者的已选目标替换为另一个已选对象，断言 `NodeDiff.references`
同时删除旧 Node 端点并添加新 Node 端点。结构变体将 `Shape` 的 `Removed` 字段替换为
`Added`，断言父 Type 的 `NodeDiff.contains` 两项变化及字段节点的 Added/Removed。
每个快照也断言 `Unknown` 调用的具名 Unresolved：Go/C/C++ 使用函数参数调用，
C# 使用委托参数调用，Java 使用静态模型中缺失的绑定；不能猜测未知目标。
所有变体均在每次分析前后比对仓库相对路径与原始字节。C17 的顶层函数没有源码父对象，
因此“子正文不污染祖先”在有父节点的 Go、Java、C#、C++ fixture 中直接检验；
C17 仍验证正文修改只落在该函数自身。

本套件覆盖真实图、三维 diff 和未知调用。取消、异常退出、进程树、协议畸形及拒绝越界
依赖现有公共 Host 专项测试；目标平台隔离、支持包入口与跨系统发行验收需另留
证据。任何一项没有在对应平台实际运行，均记录为待验，不推断通过。

## 2026-10-03 macOS ARM64 初始实测（四快照版本）

使用本地构建并安装的四个支持包清单
`/private/tmp/narrant-native-packs/native-helpers-config.json` 执行上述完整命令。
实际结果为 6 passed、0 failed：配置完整性、Go、Java、C#、Clang C17 与
Clang C++20 均经过 `NativeLanguageAnalyzer → CodeGraph → CodeGraph::diff`。
本轮中 Go 的 token 投影曾把普通换行误判为正文变化，Java 的 extensions 键曾
违反 Host 命名空间约束；各 helper 修复并重建本地支持包后，整套测试通过。
聚焦 Rust 集成测试编译、`rustfmt --check` 与 `git diff --check` 也通过。

本轮没有 Windows x64 实机结果；该平台须用同一套件和对应 Windows 支持包再运行。
本地支持包是开发验收产物，不代表远端发布。完整取消/进程树、目标隔离和支持包
安装验收由对应专项测试与 Task 7 总记录提供证据。

## Review fix round 1：六快照增强

初始六测试通过记录对应四快照版本，不能直接用于证明新增关系/结构 diff 和未知调用。
增强版本让每种模式分析六份独立仓库，并进一步加入第七个 mixed 测试。最终修复构件的完整运行结果如下；Windows x64 仍未执行，不将本机结果推广到 Windows。

## 2026-10-03 最终修复快照的本机验收

历史快照 `5e743c85377614f2adc894d633374b143d5f77b5` 曾完成生产安装
1 passed（122.34秒）和完整七测试 7 passed（8.83秒）。其 Go/Java/Clang/C# 构件源码分别为
`228f340`、`beeb5f1`、`fcbf1de`、`879c7bc`，这些结果仅证明当时的快照。
随后 `ea2f28b` 刷新 Go、Java、C#，在仅增加语言证据文档的 HEAD `feb0c38` 上通过一次
四包安装（1 passed，120.90秒）；该轮尚未运行七测试，就发现 Go 排除测试文件的残留问题。
两轮原始结果、构件摘要与源码摘要分别保留为 `/private/tmp/narrant-native-packs/` 下
`final-local-{results,artifacts,source-snapshot}-5e743c8.json` 和同名 `-ea2f28b.json`。

最终生产源码及实际测试 HEAD 均为 `e1b1586b15fa31d9139177dfa06837b63def45ed`，
包含 `ea2f28b73a5eea03e8f00519d439930bb1e5fae1` 的 Host/进程修复，以及 Go 排除
`_test.go` 的更正。最终只重新编译 Go；Java 和 C# 保留刚从 `ea2f28b` 构建的产物，
Clang 保留源于 `fcbf1de`、在 HEAD `bd62783c8f6b8382b4e25e86e8bfd684f697b760`
构建的已验证产物。新包经过实际归档、摘要生成、解压和 ready 身份检查；保留包的
归档和 manifest SHA-256 均与上一轮一致。Go 解压后可执行文件 SHA-256 为
`7c55e89ad0e3c89e55ca4fd15c571f9effd3da8f228ea677e08a808e11f6639d`。

| 包 | helper 源码提交 | 归档字节数 | 实际归档 SHA-256 |
| --- | --- | ---: | --- |
| go | `e1b1586b15fa31d9139177dfa06837b63def45ed` | 29,887,107 | `1c3bafd740b337e5ccec75dd586d5260fd3183d410a3720dd5525f1be3761822` |
| java | `ea2f28b73a5eea03e8f00519d439930bb1e5fae1` | 197,567,728 | `b9874524cef38a8886760fdbe0b27a416017e6168e458bbcc6e1b55fb80537a6` |
| c-cpp | `fcbf1de9fdd282ceaddd7cb1d34747b3b34a0cbb` | 51,814,086 | `24e5882a6637af11d386e69962aa55f6f378adc19f1ff7a8541a8382ca7631d0` |
| csharp | `ea2f28b73a5eea03e8f00519d439930bb1e5fae1` | 51,218,557 | `ceee25ba5654a0b47699ee3b6aa9b72c4909cf283407cf483d616743031f6d57` |

完整源码文件摘要保存在 `/private/tmp/narrant-native-packs/final-local-source-snapshot.json`，
归档与 manifest 摘要保存在 `final-local-artifacts.json`，最终结果为 `final-local-results.json`。
测试前后核对四个归档、manifest、运行配置及生产源码均未改变。并行修改仅涉及 Java
Python 测试驱动的超时处理，不参与本轮包构建或 Rust 图验收；其前后摘要单独记录在结果 JSON。
配置路径仍为 `/private/tmp/narrant-native-packs/native-helpers-config.json`，配置描述的路径和平台
未变，SHA-256 为 `13e3217689cf849967954cf40e1acbe822d7b52b8b11ca498e1fc10508ad9b3a`。
图测试文件 `src-tauri/tests/native_helper_graphs.rs` 的 SHA-256 为
`129ca8693fb52b2c73c7e6dc31b8d89c3cecb66418670760f0ca3fe4b2263f5c`。

```sh
# cwd 为仓库根；PATH 前置 Node24.18.0 与 Cargo。
NARRANT_NATIVE_PACKS=/private/tmp/narrant-native-packs \
  scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml \
  --lib real_native_packs_install_and_analyze_through_production_factory \
  -- --ignored --nocapture

NARRANT_NATIVE_HELPERS_CONFIG=/private/tmp/narrant-native-packs/native-helpers-config.json \
  scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml \
  --test native_helper_graphs -- --ignored --nocapture --test-threads=1
```

生产安装测试：**1 passed、0 failed、0 ignored，127.63秒**。同一测试要求四个真实归档全部存在，
逐包执行生产下载校验、安装、工厂非空图、租约删除保护和生产 origin 重启复用。
日志为 `/private/tmp/narrant-task7-e1b1586-install.log`。

完整增强套件：**7 passed、0 failed、0 ignored、0 filtered，8.96秒**，没有缺工具跳过。
配置完整性、Go、Java、C#、C17、C++20 与 mixed 场景全部通过。
五种单语言模式均运行 before/changed/formatted/relocated/relations/structure 六个独立快照，
包括具名 Unresolved、关系增删与包含增删。mixed 场景让同一仓库根中的 Go、Java、C#、C++
分别经真实 helper/Host 构图，再用生产 `CodeGraph::merge` 合为一张图；验证 before/changed/relocated
的跨语言合并、身份和 diff。日志为 `/private/tmp/narrant-task7-e1b1586-graphs.log`。
构建日志为 `/private/tmp/narrant-task7-ea2f28b-build.log` 与
`/private/tmp/narrant-task7-e1b1586-build.log`。

本次只验证 macOS ARM64 与 SDK27.0。Windows x64 和 CI SDK15.5 尚未实际运行，不能由本机
结果推断通过。没有重复全库 Rust 测试，没有手动 quality-gate、远端发布、push 或 PR。

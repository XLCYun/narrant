# 原生 helper 真实图验收

`src-tauri/tests/native_helper_graphs.rs` 是通过公开 `NativeLanguageAnalyzer::analyze`
和生产 `CodeGraph::diff` 运行的端到端测试。默认 `cargo test` 将它们标记为 ignored，
因为测试需要已经固定的真实 helper 可执行文件和目标标准库目录。测试没有假 helper，
也不把 helper CLI JSON 当作最终验收图。

先将四个 helper 及其目标目录放在分析 fixture 仓库之外，写一个 JSON 清单。路径必须是
绝对路径；`trusted_roots` 包含可执行文件所属目录，且这些目录不能与测试创建的临时仓库
重叠。`platforms` 的每条记录直接对应公开 Host `Platform`，字段和值由支持包构建过程
产生，不在测试中探测本机默认 SDK。Java 可执行文件通常是私有 JDK 的 `java`，其 `args`
需包含固定 classpath 和主类；其他 helper 通常没有参数。

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
  "csharp": {"executable": "/absolute/pack/bin/narrant-csharp", "args": [], "environment": {}, "trusted_roots": ["/absolute/pack"], "platforms": []},
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

一轮完整执行包含 Go、Java、C#、Clang C17、Clang C++20 五种模式。
`all_required_helpers_configured` 会使缺失任一 helper 配置的完整执行失败；
单独调试 Go 时可用 `-- --ignored go_real_helper_graph_and_diff` 聚焦运行，
这种聚焦结果不能当作完整验收。

每种 fixture 都有已选调用者、已选跨文件目标和未选的仓内目标。断言从实际
`CodeGraph` 根遍历节点，再核对 `code.calls` 的 Node 端点、Repository External
描述和目标身份，不依赖后端的私有 ID 编码。before/changed/format/relocated 四份
独立仓库验证子对象正文变化、普通注释与排版不变更、迁移绝对路径后的稳定身份，
并在每次分析前后比对仓库相对路径与原始字节。C17 的顶层函数没有源码父对象，
因此“子正文不污染祖先”在有父节点的 Go、Java、C#、C++ fixture 中直接检验；
C17 仍验证正文修改只落在该函数自身。

本套件只覆盖基础真实图和 diff。取消、异常退出、进程树、协议畸形及拒绝越界
依赖现有公共 Host 专项测试；目标平台隔离、支持包入口与跨系统发行验收需另留
证据。任何一项没有在对应平台实际运行，均记录为待验，不推断通过。

## 2026-10-03 macOS ARM64 实测

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

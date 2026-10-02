# narrant-clang

真正的 Clang 21.1.8 LibTooling helper。`src/main.cpp` 直接使用 `ClangTool`、AST visitor、
`SourceManager`、`Lexer`、`PPCallbacks` 和 USR 绑定证据；不运行 clang AST JSON 子进程。
输入/输出为 `narrant.native-analyzer/1` 的单请求 NDJSON，生产调用不需要额外参数。

## 固定开发环境与构建

macOS ARM64 的固定官方 SDK 及校验和见 `toolchain.json`。SDK 只用于构建 Narrant helper，
不下载被分析项目依赖。将官方 archive 校验并解包到隔离目录后：

```sh
native-helpers/clang/build.sh /absolute/LLVM-21.1.8-macOS-ARM64
NARRANT_LLVM_ROOT=/absolute/LLVM-21.1.8-macOS-ARM64 python3 native-helpers/clang/tests/test_helper.py
```

产物为 `build/narrant-clang` 和 `build/runtime/lib/libclang-cpp.dylib`。公开发布包按
`bin/narrant-clang`、`runtime/lib/libclang-cpp.dylib` 放置；二进制只使用 executable-relative
rpath，不含开发机绝对 SDK 路径。resource headers 由独立 `clang-resource` 平台提供。
`build/` 被忽略，不提交二进制、动态库或下载的 SDK。当前构建与运行验收为 macOS ARM64；
Windows 增量由专门原生 CI 验收，不能把本机构建成功当作 Windows 成功。

## 目标 catalog

- `clang-target`：`root` 为批准的目标 SDK，`version` 为 SDK 版本；metadata 精确包含
  `target_triple`、`c_standard: "c17"`、`cpp_standard: "c++20"`、
  `system_include_directories`（root 相对目录数组）、`resource_platform_id`。
- `clang-resource`：`root` 为固定 LLVM 的 `lib/clang/21`，`version: "21.1.8"`，metadata 为 `{}`。
- 两个来源分别入 evidence，禁止为了共同根目录而批准 `/` 或复制目标头文件到 scratch。
- C++ `c++` 路径组件下的包装头优先，然后 resource/include，再目标 C headers；其它读取被
  VFS 拒绝。实际验收包含 MacOSX27.0 SDK 的 C `<stdint.h>/<stdio.h>` 和 C++ `<vector>`。

## 明确的配置边界

本侧根目录必须有 `compile_commands.json`；仅接受 `arguments` 数组，拒绝 `command`、wrapper、
response file、插件、modules、自动配置、未知参数与非批准 include/sysroot。每个 TU 的 C17 或
C++20、宏、include、target 都分别解析；绝不执行 CDB 中的程序、CMake、query-driver 或构建脚本。

同一请求可以含 C 与 C++ TU。成功解析为 C 的共同 header 声明统一标为 `c`；只在 C++ 上下文
出现的声明标为 `cpp`。归并仍校验原始声明、类别、native kind 和父级；不因语言模式生成重复
header。普通 C record/C++ record 的同一源 struct 使用 `StructDecl`，不能把 AST 实现类名差异
误当成源码对象冲突。真正不能归并的源码映射返回 `union_conflict`。

源片段先按当前快照物理位置关联，再生成相对文件、词法 owner、名字、原始参数类型槽、
声明/定义角色及重复槽序号组成的 ID。位置和 USR 不进入生产 ID。prototype 与 definition
分别保留，唯一已证实定义可以成为调用目标；未决调用不按同名猜测。宏定义是 extension，
宏引用来自真实预处理展开回调；宏内容变化参与比较。模板只保留显式源码声明，不添加隐式实例。

测试脚本创建自身临时源码并核对分析前后完整文件摘要；helper 从原位置读取。取消在分析线程
之外读取，VFS/AST 检查取消和总期限；Host 仍负责在宽限过后强制回收无法协作中止的原生解析。

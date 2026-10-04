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

产品 Host 从本机开发工具发现目标 SDK，并读取其实际版本及头文件目录；不再限定 macOS SDK
版本白名单，也不要求平台指纹。缺少标准头仍明确失败，helper 只读取 Host 传入的具体根。
CI 的固定 SDK 是复现测试样本；用户不必安装相同版本，也不会随支持包下载系统 SDK。

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

## Windows 增量与附加标准头

Windows 构建入口已提供，须在实际 Windows 工具链中验证：

```powershell
./native-helpers/clang/build.ps1 -LlvmRoot C:/isolated/clang-llvm-21.1.8
$env:NARRANT_LLVM_ROOT = 'C:/isolated/clang-llvm-21.1.8'
$env:NARRANT_CLANG_HELPER = 'C:/repo/native-helpers/clang/build/narrant-clang.exe'
$env:NARRANT_WINDOWS_SDK = 'C:/Program Files (x86)/Windows Kits/10/Include/10.0.26100.0'
$env:NARRANT_MSVC_ROOT = 'C:/Program Files/Microsoft Visual Studio/2022/Enterprise/VC/Tools/MSVC/14.44.35207'
python native-helpers/clang/tests/test_helper.py
```

CMake 固定 LLVM 21.1.8，优先 clang-cpp 导出目标，否则链接官方静态 Clang 依赖图；关闭 RTTI、
开启 helper 自身异常及 UTF-8 源码。输出固定为 `build/narrant-clang.exe`；若 SDK 提供动态库，
CMake 将声明的运行 DLL 放在程序旁边，发布包必须保留这种加载关系。

`clang-target.metadata` 新增可选 `additional_system_platform_ids`，只能引用 catalog 内
`kind: "clang-system-headers"`、`metadata: {}` 的精确批准根。Windows SDK 的 `Include/<版本>`
与 MSVC `<版本>/include` 因此分别授权，无需放宽到 Program Files。附加平台 ID 进入 evidence。
空/重复/错误种类/缺失引用会拒绝；每个 TU 只读取自身目标引用的这些根。MSVC 标准头先于 resource
与 SDK C 头。直接调用和标准宏的上下文都通过同一 VFS。

macOS CI 可设置 `NARRANT_MACOS_SDK` 指向固定 Xcode SDK，测试读取 SDK 自己的版本元数据。
不设置 Windows SDK 环境时，该测试会明确 skip；这不是 Windows 验收通过。

## 审查修复第一轮

原始 token lexer 必须处理已产出的最后一个有效 token，不能把其 EOF 返回值当成“没有 token”。
优化白名单 `-O0/-O1/-O2/-O3/-Os/-Oz/-Og` 原样传入 Clang，保留 `__OPTIMIZE__` 等语义。
LinkageSpec 签名只保留 `extern` 与语言字符串。共用 Var/Field 声明通过 AST 基础 TypeLoc 取得
共享声明前缀，然后分离每个 declarator；前一个 initializer 不进入后一个节点的 code/signature。
这支持已验证的全局多变量、类字段、指针及数组声明，父节点仍只保留独立子对象锚点。

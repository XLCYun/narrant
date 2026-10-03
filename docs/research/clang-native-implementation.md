# Clang 原生 helper 实施与验证

2026-10-03，Task 5，基线 ea868a2。状态：真实 macOS ARM64 LibTooling 实现及 15 项原生集成
测试通过；这不是原先 AST JSON 探针的重命名，也不是全平台/任意 C++ 工程支持声明。

## 工具与原生 API

固定 [LLVM 21.1.8 官方 release](https://github.com/llvm/llvm-project/releases/tag/llvmorg-21.1.8)，
macOS ARM64 archive SHA-256 为
`b95bdd32a33a81ee4d40363aaeb26728a26783fcef26a4d80f65457433ea4669`。
来自官方 GitHub asset digest，下载后本地 `shasum -a 256` 完全一致；隔离解包到
`/tmp/narrant-clang-native/LLVM-21.1.8-macOS-ARM64`，没有全局 brew install。

实现遵循 [LibTooling](https://clang.llvm.org/docs/LibTooling.html) 的 `ClangTool` 与
`FrontendAction` 路径；CDB 格式参考 [JSON Compilation Database](https://clang.llvm.org/docs/JSONCompilationDatabase.html)，
但只接受安全子集的 arguments 数据，不接受可执行 command/wrapper。直接访问原生 AST、
SourceManager、Lexer、PPCallbacks、USR；没有分析项目的编译/链接或 AST JSON 子进程。

`libclang-cpp.dylib` 约 103 MiB，官方库已包含 LLVM 符号，仅依赖系统 framework、libSystem、
libz 和 libc++。构建关闭 RTTI 以匹配该发布包；helper 只含相对 runtime rpath。当前二进制
SHA-256 `1c6c2ca2bc1218c6dfbe0bc06cce09a3d4af03376d7b2e39793905e129d4cba0`，
库 SHA-256 `d120fa76bf73c3bfe5516fb87948cd67d99e9c349b7e4ad89bccb90107081c4b`。
这些是本次开发产物证据，不是远端发布承诺。

## Catalog、配置及只读边界

SDK 与 resource 分开授权，字段、根目录与运行命令见
[helper README](../../native-helpers/clang/README.md)。MacOSX27.0 实测使用
`arm64-apple-macosx27.0`，include 为 `usr/include/c++/v1` 与 `usr/include`；resource 来源为
独立已校验 LLVM 21.1.8。`-nostdinc/-nostdinc++` 禁用宿主隐式搜索，SDK/include 路径由 metadata
重建，VFS 对每次打开/状态查询解析真实路径，只开放该 TU 的目标目录和本侧源码。

项目运行时不读取父目录 CDB、用户 clang config、响应文件或已安装第三方目录；不下载依赖。
本侧 `compile_commands.json` 全部 TU 上下文按顺序解析，选集仅决定图成员。选中文件必须被
实际 TU SourceManager 覆盖；缺头、孤立选中 header、语法错误、未知必要参数都明确失败。
当前不猜测 CDB 缺失时的 standalone 设置，也不声称穷举没有出现在 CDB 的宏组合。

所有编译器启动 token 只校验为已知 driver 名，绝不调用其路径。插件、modules、-Xclang、
query-driver、--config、response files 拒绝；`-c/-o` 只消费数据，实际 frontend 是纯 AST action。
仓内 vendor/node_modules/.git/.cache/.venv/target 排除。已有缓存、仓外链接不因此变成已批准依赖。
当前验证是 API 来源控制及前后摘要，并不声称它等同通用操作系统沙箱。

## 映射、源码身份与并集

AST 的物理位置只作为单次快照中的归并证据。公开 ID 用相对文件、词法父级、名字、原始参数
类型槽、声明/定义角色与重复声明槽顺序，不含 checkout 前缀、行号、偏移、Clang 指针或 USR。
参数名不进入重载槽，普通重载重排不变 ID；完全相同的重复声明仍按源码声明槽序号区分。
prototype/definition 分别保留；USR 只帮助已绑定调用关联同一实体的唯一源定义。

Code 与 signature 来自原始 Lexer token，删除注释/普通排版而保留字面量和预处理边界。
独立子声明在父内容中换成身份锚点；source 保留原始完整片段，仅供展示。宏定义是明确的
MacroDefinition extension；PPCallbacks 证实的展开产生 code.references，因此改宏数字正文
不会丢失内容变化。宏调用造成同一原文的推导返回类型不同，不把展开类型强塞入源码签名。
同一片段成为 Type/Value 或 Field 的父级从 Left 变 Right 时，整次返回 union_conflict。
多个源片段同名保留独立对象；隐式模板实例和编译器合成对象不复制成源码节点。

每个 TU 独立保留 C/C++ 模式。混合项目的普通共享 header 只生成一份兼容源声明；若它在合法
C 上下文出现则规范为 c，否则为 cpp。普通 struct 的原生种类规范为 StructDecl，避免因
RecordDecl/CXXRecordDecl 实现类差异伪造冲突。类别、源码范围、父级和内容不兼容仍失败。

最终图完成后才分类 Node/External/Unresolved；External 附精确 repository/dependency
descriptor。函数指针保留未知，某配置确定绑定不会删除另一配置的未知观测。

## 实测与保留的失败修复证据

命令：

```sh
native-helpers/clang/build.sh /tmp/narrant-clang-native/LLVM-21.1.8-macOS-ARM64
python3 native-helpers/clang/tests/test_helper.py
```

最终输出 `Ran 15 tests in 5.059s / OK`。测试内含 C17/C++20 两模式的五种配置并集：普通调用
合并、同名分支分离、宏返回类型兼容、类别冲突和父级冲突；另外覆盖跨 TU/header、重载、
源声明/定义、未知关系并存、SDK 标准头、mixed shared header、宏正文/续行、模板/匿名对象、
源码排版/子对象内容、恶意参数、缺 header、孤立 header、符号链接/vendor、重复 JSON 键、
取消、stdin EOF、结果限额与 completed 后 stdin 保持开启时仍自行退出。每次分析核对仓库文件
摘要及目录集合，没有新文件或内容变化。配置倒序结果图相同。

开发过程真实失败均有对应修复/回归，没有删掉断言迎合结果：

1. 官方 LLVM 没有独立 libLLVM.dylib，且禁用 RTTI；改为直接链接 self-contained clang-cpp，
   不意外链接 SDK 附带的另一份 libc++，实际 otool 检查仅系统依赖。
2. 异常消息原先把临时 `what()` 指针放进 LLVM JSON 非拥有字符串，边界错误出现 UTF-8 损坏；
   改成拥有的 std::string，symlink/vendor 错误回归通过。
3. 缺失 include 候选原先返回 permission_denied，提前中断标准头搜索；改为不暴露未批准路径的
   no_such_file_or_directory。SDK C++ 进一步暴露 `<cstddef>` wrapper 次序错误，固定为
   C++ wrapper → resource → C headers。真实 std::vector smoke 才最终成功。
4. 初始 CDB 单模式限制已移除；mixed C/C++ 共享 header 测试正序/倒序得到同一图。
5. 添加宏定义对象时与 Clang 的 MacroUse enum 同名产生编译错误，重命名 SourceMacroUse 后
   原生编译和完整套件通过；宏正文变化和续行另有独立回归。

## 尚未完成的范围

Windows 构建、Windows 进程/路径原生验收由后续独立增量负责，本报告不以 macOS 代替。
未提供 modules/PCH、任意驱动器参数、framework 搜索目录、任意第三方 SDK 或任意 C++ 方言支持。
构造/直接调用和宏引用已有验证；完整继承/type-use/所有语言隐式调用词汇尚未纳入本次矩阵。
大型真实工程性能、极端宏一处生成多个声明、复杂多 declarator/非连续范围、完全同名重开作用域
的跨编辑稳定性仍需扩大样本；不能把这 15 项夹具当作所有 C/C++ 语法的完整证明。

没有运行 Cargo、quality-gate、Git 提交或远端发布。

## 275313e 之后的 Windows 增量与审查修复第一轮

此节覆盖独立审查 `.superpowers/.../task-5-review.md` 的全部 4 个 Important，保留前文快照事实。
新增 CMakeLists.txt/build.ps1、Windows PeekNamedPipe/ReadFile 取消读取、跨平台环境清理、显式
UTF-8 filesystem 路径；新增 `clang-system-headers` 与可选 additional_system_platform_ids，
附加根分别校验、按目标限定读取并写入 evidence。MacOS 标准头测试也接受固定 SDK 环境变量。
Windows 官方 x64 dev archive 的 URL/hash 及 LLVM/Clang 精确许可证文件、URL/hash 记录于
`native-helpers/clang/toolchain.json`；没有用 installer 代替开发 headers/libraries。

新增的四个审查定点测试在旧二进制运行得到 `Ran 4 tests in 0.850s / FAILED (failures=4)`：

- 单 token 宏 `NUMBER 1→9`、enum initializer 数字变化被末尾 token bug 吞掉；修复 lexer 循环，
  先处理有效 token 再处理 EOF，补全原始 token 序列。
- `-O0/-O2` 下的 `__OPTIMIZE__` 分支都变成 plain；安全优化白名单现原样进入真实 Clang。
- 有/无花括号的 `extern "C"` 父 signature 混入子函数正文；现以语言字符串结束位置确定自身头部。
- `int a=1,b=2` 的 b 混入 a initializer；现用原生 TypeLoc 的基础类型范围识别共享前缀，再把
  每个 declarator 的片段分开。全局、字段、指针、数组均回归，字段父内容保持独立子锚点。

修复后相同定点命令输出 `Ran 4 tests in 2.515s / OK`。
完整命令 `python3 native-helpers/clang/tests/test_helper.py` 输出
`Ran 22 tests in 4.304s / OK (skipped=1)`：21 项实际通过，Windows SDK 实测因无 Windows 根
明确跳过；不能计为 Windows 通过。macOS SDK、独立附加根的真实头文件/宏/来源证据、Unicode
文件名、前述 15 项基础行为及四项审查回归均实际执行。

该轮 macOS binary SHA-256：
`4e84eb0ffad0c28f3d13eccb3ebf3b9d4e10d66c97bee641ab7d3f97f8bc0d00`。
构建仍为 `native-helpers/clang/build.sh /tmp/narrant-clang-native/LLVM-21.1.8-macOS-ARM64`，exit0。
Windows CMake 与 PowerShell 分支仅已编写，尚待实际原生 CI；没有把宏定义源码对象、静态调用和
有限标准头测试扩大宣称为所有 C++ 关系或大型项目完整性证明。此轮未运行 Cargo/quality-gate。

## 审查修复第二轮：成员指针声明前缀

第一轮复审确认 token/优化参数/LinkageSpec 已修复，发现多 declarator 的残留：
`int C::*a=nullptr,b=2;` 的 C:: 仍被误判为共享类型前缀。新增真实回归首先得到
`Ran 1 test in 0.222s / FAILED`，b.signature 为 `int C :: b = 2 ;`。

现逐层使用原生 `TypeLoc::getLocalSourceRange` 定位 declarator；`MemberPointerTypeLoc`
包括完整限定名，不再从第一个星号猜起点。简单/嵌套限定名以及成员指针数组实际通过，
改变 a 的 C/D 成员指针类型时，b 的 signature/code 仍为 `int b = 2 ;`，ID 不变。
受影响聚焦套件包含新增回归、普通多声明、C/C++模式和两语言五种并集，输出
`Ran 4 tests in 1.370s / OK`；普通多声明另补 C17 全局/字段数组样本。
本轮二进制 SHA-256：`e8789931d8ed2b6d4782779b1ffa980dc75023eaeee180894af637ce7668b46f`。
未重复全套或跨平台验收，未扩展复审 deferred 的匿名 typedef 范围。

## 最终平台结果（2026-10-03）

`4c65bb8` / [运行37111018854](https://github.com/XLCYun/narrant/actions/runs/37111018854)
已完成该helper及公共新native链路的macOS ARM64/Windows x64验收，含真实安装和全部8项公共图测试。
精确语言/SDK、计数、日志和未覆盖范围见[跨平台记录](native-crossplatform-validation.md)。
总CI仍因旧共享WindowsJob回归失败；本轮未修改其行为，未发布支持包，也不扩大前文语言支持范围。

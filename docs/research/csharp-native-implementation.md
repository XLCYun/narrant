# C# 原生 helper 实测与固定边界

日期：2026-10-03。对应 `native-helpers/csharp/`，profile 为 `csharp-static-v1`，投影版本为
`csharp-source-v1`。本文件记录已经运行的实验及受支持范围，不将 C# 14 编译器能解析的所有
项目模式一并称为产品支持。

## 固定工具与引用来源

开发机初始 `PATH` 中没有 `dotnet`。从微软官方
[.NET 10 release manifest](https://dotnetcli.blob.core.windows.net/dotnet/release-metadata/10.0/releases.json)
取得 10.0.100 macOS arm64 SDK 的固定地址，下载到 `/tmp`，以 manifest SHA-512 校验后
解压到独立目录；没有全局安装。归档 URL 是
`https://builds.dotnet.microsoft.com/dotnet/Sdk/10.0.100/dotnet-sdk-10.0.100-osx-arm64.tar.gz`，
SHA-512 为
`0ee9c774f9c63601844619b67d6ca829d70fa26b9949b808e801380c35edb12e4c9418baf8e2ef8ba1dc6dc7d307dbf5830fb0f4f97666032a391c5c097ee607`。
固定版本安装方法符合[微软 dotnet-install 文档](https://learn.microsoft.com/en-us/dotnet/core/tools/dotnet-install-script)；
这里直接用归档和 manifest 摘要，避免安装脚本选择浮动版本。

SDK 自带 `Microsoft.CodeAnalysis` 与 `Microsoft.CodeAnalysis.CSharp` 程序集版本均为
**5.0.0.0**，二者 SHA-256 分别为
`b1b6c8b56c6902d7269056b419effc79da9460a95b7ab40fe1ee0e55bc250bc1` 和
`c975b490fa73bb3ee493c4a16de622fb97329e8f362f0014e03bbd32726edd3f`。
SDK 带 .NET runtime 10.0.0 与 `Microsoft.NETCore.App.Ref/10.0.0/ref/net10.0`。
helper 运行时的 runtime 和被分析项目的 target reference assemblies 是两个不同输入：
运行时随支持包，目标引用目录由 Host 的 `dotnet-reference-assemblies` Platform 授权。
Roslyn 使用 `CSharpCompilation.Create`、
[`MetadataReference.CreateFromFile`](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.metadatareference.createfromfile)
读取这些 reference DLL；它没有调用 `MSBuildWorkspace`、项目的 `dotnet build`、`restore`、
generator、analyzer 或项目自定义 target。微软的
[reference assemblies 说明](https://learn.microsoft.com/en-us/dotnet/standard/assembly/reference-assemblies)
也明确区分了编译引用与运行实现。

## 已实现的静态项目范围

`narrant.csharp.json` 显式列举文件、C# 14、net10.0、常量与 implicit usings；多个明确
配置分别建 Compilation，最后按源码身份合并对象及确认的关系。普通 SDK-style `.csproj`
只接受字面 `Microsoft.NET.Sdk` 和 README 所列的有限属性、默认 `**/*.cs`、字面
`Compile Include/Remove` 与 `ProjectReference`。仅对
`TargetFramework`、`Configuration`、`Platform` 的字面 `==` 或 `!=` 求值；默认 Debug / AnyCPU，
显式 Debug/Release 等值或不等值分支分别建 Compilation 后并集；仓内 ProjectReference
沿用引用方配置重建被引用项目。默认源码发现排除隐藏目录，
`ImplicitUsings=true` 与 `enable` 同样启用隐式导入，并按固定 SDK 补全 net10 预处理常量。
条件中未纳入的显式文件报 `inactive`；可靠的 `test_only` 或 `IsTestProject=true` 可报
`excluded_test`。不读取 NuGet cache 或已生成的 obj/assets。

编译单元包含未选中的仓内源码作绑定上下文，图成员只来自选集。通过原始相对文件路径、词法
父级、kind、源码参数类型及分支标记形成身份；partial 片段逐文件保留，不能挑首个片段代表
全部声明。所有明确配置的直接子声明跨度先合并，父对象的规范内容因此同时保留 `#if` 各
分支原文，却用子对象锚点排除子方法体。规范内容按 Roslyn token 保留运算符分组和 raw
字符串字面值；分支 ID 只读取真实指令 trivia，避免注释中的伪 `#if`。关系目标在全配置对象集合形成后，依 Roslyn 的
`DeclaringSyntaxReferences` 判为 Node、仓内 External、平台 External 或 Unresolved。

已映射文件、namespace、type、delegate、record、扩展块、方法/构造/析构/运算符、
field/property/indexer/event/enum member、显式 accessor 与顶层语句。表达式体属性归 getter，
local function/lambda 留在最近外层 owner。已覆盖显式调用、对象创建、构造初始化器、方法组、
继承、接口实现、成员归属和窄范围类型使用；dynamic/缺依赖不按同名硬猜目标。
未知声明种类会失败，不静默丢节点。

## 实际验证

在隔离 SDK 10.0.100 上执行：

```text
dotnet build native-helpers/csharp/Narrant.CSharp.Helper.csproj -c Release --no-restore
  Build succeeded. 0 Warning(s), 0 Error(s)
dotnet publish native-helpers/csharp/Narrant.CSharp.Helper.csproj -c Release --no-restore -p:UseAppHost=false -o /tmp/narrant-csharp-publish
  publish succeeded; ready message observed from published helper
python3 -W error::ResourceWarning -m unittest discover -s native-helpers/csharp/tests -v
  Ran 10 tests ... OK（首次提交；修复轮次扩展到 23 项，见 Task 4 报告）
```

测试是独立进程 NDJSON 会话，覆盖 partial/重载和精确调用、跨 ProjectReference Node、
条件调用与条件声明并集、缺依赖 Unresolved、目标缺失、extension block、接口实现、
字段和事件共享声明头、同文件 partial 与接口限定 ID、泛型重命名、partial 实现调用、
外部重载身份、构造初始化器与委托未知调用、属性/事件关系、SDK 固定默认值与 Release 分支、
测试排除/启用、条件 Compile 的 inactive、格式/注释与 body-only 内容、取消、静态拒绝
Target/Import 及分析前后仓内文件 SHA-256 一致。测试中的项目 target 含写入 marker，
helper 返回 `unsupported_configuration` 且 marker 不存在。宿主到真实 CodeGraph/diff 的
联合验收由接入任务另行记录；这里不将本 helper 单元探针当成完整应用验收。

## 当前明确边界

仅 C# 14 + net10.0 普通源码得到实测承诺。Razor、Unity `.asmdef`、脚本、旧式项目、
任意 import/target/package reference、条件 PropertyGroup、MSBuild 属性函数、显式 Compile
glob、额外 TFM、source generator 和第三方业务依赖无法由静态子集证明，直接返回配置失败。
只对固定 SDK 模式下的 Debug/Release、AnyCPU 和列出的等值条件做求值；不运行项目构建
以求解其它配置。`narrant.csharp.json` 的 `test_only` 是仓内显式声明，项目模式使用
`IsTestProject=true`，不按路径名字猜测测试。

当前测试尚未覆盖所有合法 C# 14 语法及引用边界组合，包括 file-based app 指令、所有
LINQ/collection initializer 的隐式调用、通用运算符绑定、完整跨平台源码编码和大型项目
性能。遇到未知声明或不能兼容归并的对象时 helper 失败；不能据此推断所有未测语法已支持。

## 最终审查修复补充（2026-10-03）

以上为此前快照。新增真实 helper 回归先复现接口继承被误报为 implements，
再修正为同时检查源声明种类：接口继承接口输出 code.extends，类/结构体实现接口仍为
code.implements。回归逐项断言 IBase/IDerived 的准确 Node 目标及唯一关系语义。

在 native-helpers/csharp 中使用既有 .NET 10.0.100 执行
`DOTNET_ROOT=/tmp/narrant-dotnet-10.0.100 DOTNET_CLI_HOME=/tmp/narrant-dotnet-home /tmp/narrant-dotnet-10.0.100/dotnet build -c Release --no-restore`
成功，0 warning / 0 error；仓库根目录
`python3 -m unittest native-helpers/csharp/tests/test_helper.py` 为 24 项通过。
当前 bin/Release/net10.0 产物来自修复后的源码；支持包刷新及公共 Host 验收由后续整合
执行，尚未声称 Windows x64 已真实运行。

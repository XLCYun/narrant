# 项目目标版本发现与验收

2026-10-04，相对 `c379e0c9fbb47e4551e2e21b7d90a25379d93f24` 清理 Java、Go、C/C++ 的项目版本门禁。
这是本轮用户请求的实现记录。既有 Rust、TypeScript/JavaScript、Python 分析策略不变；C# 与系统 SDK
发现已在上一轮完成，见[跨平台记录](native-crossplatform-validation.md)。本轮最终跨平台验收待补记。

## 选择规则

| 输入 | 当前规则 | 保留的明确边界 |
| --- | --- | --- |
| Java release | Host 从显式路径、常见 JDK 安装和 PATH 线索只读发现；同 major 取最高稳定补丁，每份配置精确匹配 release | JDT 公开能力 API 决定语言上限；Java 8 经典标准 JAR 与现代模块镜像分别加载，不扫描扩展或业务 JAR |
| Go go/toolchain | 从本机 SDK VERSION 与完整源码布局发现；有效声明同 minor 内选满足最低 patch 的最高稳定补丁 | 不用新版 SDK 替代旧 minor；编译器能力来自实际 Go 构建版本；不执行 go list、go env 或下载工具链 |
| Go workspace | go.work 的 go/toolchain 生效，go 不低于各成员；无 workspace 的独立 module 分开分析 | 每包语言规则来自自己的 go.mod；本地 replace 也需要 go.mod，不把同目录 module 自动当成 workspace |
| C/C++ 标准 | 每条 CDB 命令明确提供 -std，LLVM LangStandard 识别稳定标准、GNU 方言及别名 | 缺 CDB/标准、语言模式错配、未知或预览标准明确失败；不运行命令、wrapper 或项目构建 |

Java 的目标 `version` 必须等于 JDK release 文件的 `JAVA_VERSION`。运行 JDK25 只可作为 Java25
目标，不可冒充 Java8/17。Go 的 release tags 与默认实验开关来自所选 SDK；静态读取器仅解释
SDK 默认布尔/字符串表达式、声明及目标 switch，遇到未知形式或定制实验默认值则失败。
这是避免旧标准库混入新编译器默认设置所需的读取，不是通用 Go 解释器。

Go 的公开 `types.Error.Msg` 中明确的语言版本错误必须终止提取，包括必要仓内导入包的函数体，
不能与缺失依赖一起降为未知关系。固定 Go1.27 的 types 已删除 Go1.23 泛型别名实验开关检查，
因此只对实际选中的 Go1.23 SDK 补回该默认开关约束；新 SDK 下仍服从原生语言规则。
这是已复现的编译器兼容差异，不扩展成另一套语言特性框架。
仓内包缓存独立记录函数体检查与测试文件覆盖；导入包先进入不含测试的缓存后，用户选中的
测试文件必须重新加载，不能因已检查函数体而漏掉对象或版本错误。

缺少目标返回 `target_unavailable`，超出编译器或静态配置能力返回 `unsupported_configuration`。
第三方声明缺失仍允许关系未知；语法或结构无法完整提取时失败，不返回伪成功的部分图。
配置并集与持久身份规则不变。较新的 SDK 被发现不意味着该编译器已支持它。

## 构件与测试样本

Java 和 Go 包不再附带另一份固定项目标准库；Java 保留私有工具 JDK，Go 只分发编译好的 helper。
产品发现逻辑没有 CI 版本列表。可复现构建仍固定 JDT 3.47.0、工具 JDK25、Go1.27.1、LLVM21.1.8
及 Roslyn/.NET 工具；这些构件是受控分析工具，不是项目目标白名单。

CI 另外准备真实 Java8/17/25、Go1.23.8/1.27.1；前一轮 net8/net9/net10 与系统 SDK 样本保留。
Java8 使用 Temurin8u504-b01，macOS 上仅读取 x64 archive 的标准声明，不执行其二进制；Windows
同样只把该 JDK 用作目标数据。所有官方 archive 的来源与摘要在 `resources/native-toolchains.json`。
CI 运行这些样本不要求用户安装全部样本；用户只需提供项目实际目标所需的声明。

## 本地验证进度

- Java：真实 Java8/17/25，35项通过，0跳过；含 API/语法隔离、预览禁用、多模块不同 release、同源并集与扩展 JAR 排除。
- Go：固定1.27.1编译器及真实1.23.8/1.27.1 SDK，36项通过，0跳过；含工作区优先级、独立模块、旧语言语法、旧API与实验默认值隔离，以及导入后再选中测试的缓存回归。
- Clang：固定LLVM21.1.8，32项中31项通过，1项Windows SDK测试按平台跳过；涵盖C89–C23、C++98–C++23、GNU、driver/-x及共享header多标准并集。
- 打包脚本测试24项通过；Rust聚焦32项及全量764项通过，0失败，82项固定工具或进程夹具默认ignored。
- 四包真实安装1项通过，73.70秒，覆盖多目标切换与无包内目标库；随后Go诊断与缓存修复重新打包，8项公共图/diff全部通过，0ignored，10.47秒。
- 最终代码与双平台Actions结果将在本轮完成后补记；本地成功不替代Windows原生运行。

历史实施记录中的 Java17、Go1.23.8、C17/C++20 均为当时的有限实现或样本，不再是当前产品版本政策。

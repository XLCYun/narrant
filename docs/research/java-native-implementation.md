# Java 原生 helper 实施与隔离验证

2026-10-03，Task 3。实现位于 `native-helpers/java/`，生产分析后端只有 [Eclipse JDT Core 3.47.0](https://central.sonatype.com/artifact/org.eclipse.jdt/org.eclipse.jdt.core/3.47.0)。`tools/Probe.java` 是开发探针，不进入生产入口。运行时使用 Temurin JDK 25.0.4.1+1；被分析代码的目标 API 来自**单独、精确匹配 release 的** JDK 17.0.20.1+1。目标平台 root 必须是 JDK Home，含 `release` 与 `lib/jrt-fs.jar`；JDT 实际解析需要 `lib/modules`。启动 JDK 不充当目标平台默认值。

## 固定依赖与构建

`dependencies.lock.json` 固定 JDT Core、ECJ、Eclipse Platform 运行闭包和 Gson 的 Maven 坐标及 SHA-256。`tools/fetch-deps.py` 仅从 Maven Central 获取这些**helper 构建依赖**到忽略的 `.tools/java-native/lib/`，逐项校验锁定哈希；它不读取 POM，也不解析被审仓库的业务依赖。`build.py JDK25_HOME LIB_DIR` 和 `build.sh JDK25_HOME LIB_DIR` 只编译 helper 源码，使用 `-proc:none`，并先校验 JAR。`build/classes/` 与 `.tools/` 均被忽略；不会写入被分析仓库。

启动参数为 `<runtime-jdk>/bin/java -cp <classes><pathsep><fixed-lib-classpath> dev.narrant.java.Main`。本地 pack 可在确认只含锁定 JAR 后使用 `lib/*`；macOS/Linux 路径分隔符为 `:`，Windows 为 `;` 且二进制名为 `java.exe`。v1 握手固定为 `narrant-java`、`0.1.0`、`java-source-v1`、`java-static-v1`。平台字段固定为 `kind: "java-jdk"`、`metadata: {"release":"17"}`、`root: <exact-JDK17-Home>`。`version` 与 `fingerprint` 非空，并由 Host 的目录来源与摘要校验提供。

JDK 归档来自 [Adoptium 官方发行版](https://github.com/adoptium/temurin25-binaries/releases/tag/jdk-25.0.4.1%2B1)和 [JDK 17 发行版](https://github.com/adoptium/temurin17-binaries/releases/tag/jdk-17.0.20.1%2B1)。macOS arm64 JDK 25 归档 SHA-256 为 `61979887f7506a24a57439ff99adb8b3a7fc89977d9cfe3b8984f58a981b7b9d`；JDK 17 归档为 `196d13ba5f10414bef7f6a05a9b3f00edacb18ebacef2b99485db9e2ee18f0e8`。许可证与完整包清单需随实际支持包一起审查。

## 目标 API 隔离实验

JDT `ASTParser.setEnvironment` 使用目标 JDK 的 `lib/jrt-fs.jar`，关闭运行 VM bootclasspath；同时设置 compliance 17、release enabled、preview disabled。其 [官方 API](https://help.eclipse.org/latest/topic/org.eclipse.jdt.doc.isv/reference/api/org/eclipse/jdt/core/dom/ASTParser.html)允许这样的 standalone 输入。实验说明仅设置 release 并不足以隔离 API：

| 相同 Java 17 fixture | `List.of(1)` | Java 21 `List.getFirst()` | `Helper.pick(int/String)` | 缺失 `missing.Type.nope()` |
| --- | --- | --- | --- | --- |
| 错误地指定 JDK 25 目标镜像 | 确定绑定 | **错误地确定绑定** | 两个不同确定绑定 | null |
| 指定精确 JDK 17 目标镜像 | 确定绑定 | 诊断 undefined，binding null | 两个不同确定绑定 | null |

固定 JDK 17 镜像只保留 `release`、`lib/jrt-fs.jar`、`lib/modules` 时，后一行仍成立。实验用 `tools/Probe.java` 在合成临时源码执行，不运行项目。生产 helper 还会读取目标 `release` 文件，并拒绝目录实际版本与 metadata 的 release 不一致。未来 Java 8 需真实 Java 8 目标 API 镜像与单独实验；当前明确拒绝 8/11/21/25，不能在 JDK 25 上用 17 source 选项伪装 Java 17 API。

## 静态项目模型及图

首版支持 release 17、UTF-8、非 JPMS 的两个静态入口：

1. 仓内 `narrant-java.json`，schema 必须为 `java-static-v1`，含 release 字符串 `"17"` 和 `source_roots: [{"path":"src/main/java","role":"main"},{"path":"src/test/java","role":"test"}]`。源码根有明确 main/test 归属；存在 Gradle 脚本时也只消费此已在仓内的声明文件，不执行 Gradle。
2. 仓内 `pom.xml`：解释字面 `maven.compiler.release`、仓内模块和默认或字面 main/test 目录；子模块可以继承仓内聚合 POM 的 release。外部 parent、profile、plugins、pluginManagement、repository 扩展及未知 release 拒绝。不会调用 Maven effective-model、下载业务 JAR、运行处理器或插件。业务依赖缺失只使相应关系保持 unresolved，源码对象仍完整时可成功。

源码真实路径不得越过固定仓库。每个已解析文件的 package 与声明源码根下目录对应；配置之间建立单独的 JDT parser 环境。选集控制最终图成员，源根内其余 `.java` 只用于上下文。`include_tests=false` 只排除明确 test 根的选中文件；生产类名含 `Test` 不被推断为测试。显式的 file/package/import、class/interface/annotation/enum/record、method/constructor、field/enum constant/record component、initializer、local/anonymous type 进入图，默认或生成成员不伪造成源码节点。局部变量、参数及 lambda 内容留在最近独立对象。

源码 ID 由仓内相对路径、词法 owner、种类、名字与源码参数类型槽构成；JDT binding key 只用于本次语义连接。JDT 词法器消除普通注释及排版，独立子声明在父节点自身内容中以有序锚点代替。确定、非 recovered 的绑定才可成为 Node 或 External；缺失/恢复绑定成为 Unresolved。映射 `code.calls`、`code.references`、`code.extends`、`code.implements`、`code.member_of`、`code.type_use` 和 `code.imports`。External 带仓内或目标标准库来源的 descriptor。`evidence` 完整覆盖选集文件、配置和每个节点的选中源码来源。

## 已验证边界与待扩展项

`python3 -m unittest native-helpers/java/tests/test_helper.py -v` 的 9 项合成仓库测试通过，包括跨文件重载、缺失依赖、目标 API 反例、body/child/格式变化、测试根、有限单/多模块 POM、Gradle/JPMS 拒绝、取消、期限及路径拒绝。测试用真实 JDT 和准确目标 JDK，不运行被测代码。没有在 Windows 原生运行；跨平台构建脚本和 `java.exe` 测试入口已准备，仍须 Windows CI 验证。并未证明 JDK 21/25、Java 8、JPMS、Android、任意 Maven 插件/外部 parent 或复杂 Gradle 动态配置可用，这些输入当前拒绝。仓内多模块之间尚未建立 Maven 依赖图，跨模块业务绑定可能 unresolved；不会用同名猜测补边。

详细执行记录与待审风险见 `.superpowers/sdd/0024b-native-analyzer-implementation-plan/task-3-report.md`。

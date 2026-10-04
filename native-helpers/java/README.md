# Java 静态分析 helper

该 helper 使用 `dependencies.lock.json` 锁定的 Eclipse JDT 解析源码。helper 自身的运行 JDK、JDT 工具版本与被分析项目的目标 Java release 相互独立。分析不执行 Maven、Gradle、项目脚本、注解处理器或目标 JDK，也不下载项目依赖。

## release 与目标 JDK

`java-static-v1` 接受规范整数 release，例如 `8`、`17`、`25`。支持范围直接由固定 JDT 的公开 API `JavaCore.isJavaSourceVersionSupportedByCompiler` 判定；`8` 在调用 JDT 时映射为其历史名称 `1.8`。不维护产品版本白名单，也不把未知版本降级到最高已知版本。`1.8`、带空格、前导零、属性表达式和预览版本后缀均不是合法的配置 release。

每份配置单独匹配 `environment.platforms` 中的 `java-jdk`：

- `metadata.release` 必须与该配置完全相同，同一 release 只能有一个候选平台。
- `version` 必须等于该 JDK `release` 文件中的真实 `JAVA_VERSION`。经典的 `1.8.0_XXX` 对应 release `8`；现代版本从稳定版本号读取 major。EA 版本不可用。
- Java 8 使用目标 JDK `jre/lib` 中的 `rt.jar` 及存在的 `resources.jar`、`jsse.jar`、`jce.jar`、`charsets.jar`。必须有 `rt.jar`；不扫描 `ext`，不自动加载其他 JAR。其他厂商的非经典目录布局尚不支持。
- Java 9 及以后使用目标 JDK 的 `lib/jrt-fs.jar` 与 `lib/modules`，不加入运行 helper 的 JVM 标准库。

因此，Java 25 运行时不能代替缺失的 Java 17 目标。较旧目标不存在的 API 保持未知关系；高版本语法、禁用的预览特性或畸形语法导致完整提取失败。预览始终禁用。缺少精确目标返回 `target_unavailable`，不被当前 JDT 支持的 release 返回 `unsupported_configuration`，不会附带部分图或证据。

JDK 发现与同 major 的稳定补丁版本选择由 Host 完成；helper 只验证并读取协议指定的本地 JDK。

## 多配置

standalone 声明可为不同源码根指定不同 release：

```json
{
  "schema": "java-static-v1",
  "configurations": [
    {"id": "legacy", "release": "8", "source_roots": [{"path": "legacy/src", "role": "main"}]},
    {"id": "service", "release": "17", "source_roots": [{"path": "service/src", "role": "main"}]},
    {"id": "current", "release": "25", "source_roots": [{"path": "current/src", "role": "main"}]}
  ]
}
```

有限 Maven 模型中的模块也可以分别声明或继承字面 `maven.compiler.release`。每份配置使用独立的 JDT 语义环境与 binding 索引，来源证据列出各自的平台 ID。同一物理源码可属于多份配置，最终图仍按原有身份与关系并集规则合并；某配置中的未知关系不会被另一配置的确定关系抹去。

此改动仅放开目标语言版本。JPMS `module-info.java`、动态 Maven 配置、项目插件与缺少静态声明的 Gradle 项目等既有限制仍会明确报错。

## 编译与验证

以下命令仅编译 helper 自身。运行 JDK 和锁定 JAR 必须已在本地准备完成：

```sh
python3 native-helpers/java/build.py "$NARRANT_JAVA_RUNTIME" "$NARRANT_JAVA_LIB"
python3 -m unittest discover -s native-helpers/java/tests -v
```

测试默认使用仓库 `.tools/java-native` 中既有的固定运行 JDK 25、JDK 17 与锁定依赖。可用以下环境变量覆盖真实夹具：

| 变量 | 用途 |
| --- | --- |
| `NARRANT_JAVA_RUNTIME` | 运行与编译 helper 的固定工具 JDK |
| `NARRANT_JAVA_LIB` | 与锁文件一致的 helper JAR 目录 |
| `NARRANT_JAVA_TARGET` | Java 17 目标 JDK，兼容原测试入口 |
| `NARRANT_JAVA_TARGET_8` | Java 8 目标 JDK；只读取标准库，允许与运行机器架构不同 |
| `NARRANT_JAVA_TARGET_25` | Java 25 目标 JDK，默认使用固定运行 JDK 的真实 Java 25 标准库 |

Java 8 夹具未配置且未在 `.tools` 中找到时，对应测试会明确跳过；完整跨版本验收应提供三个真实目标。`test_target_versions.py` 覆盖经典标准 JAR、8／17／25 的 API 与语法隔离、预览禁用、多配置与 Maven 模块、共享源码并集、缺失／错配／重复平台、非法或超出编译器能力的 release，以及扩展 JAR 隔离。所有项目均为临时夹具，并验证关键分析前后的文件哈希不变。

当前能力来源可直接用锁定 JAR 的 `JavaCore.getAllJavaSourceVersionsSupportedByCompiler()` 查询；编译器错误分类使用同一 JDT 的 `IProblem.Syntax`、`Compliance`、`PreviewRelated` 常量，不依赖错误消息文本。

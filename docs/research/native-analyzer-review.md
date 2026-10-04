# 原生语言分析器审查记录

整理日期：2026-10-04。交付位于[草稿 PR #200](https://github.com/XLCYun/narrant/pull/200)，尚未合并或发布。
本记录汇总已完成的独立审查、修复复核与平台验收，不代表整理文档时重新执行了一次全分支审查。
审查依据为 [0024 协议](../specs/0024-native-analyzer-helper-protocol.md)、
[0024a 语言规则](../specs/0024a-native-language-analysis-profiles.md)和
[0024b 实施计划](../specs/0024b-native-analyzer-implementation-plan.md)。

## 审查覆盖与结论

公共 Host、Go、Java、C#、Clang、应用与支持包接入分别经过审查；有发现的部分完成修复及定点复核，
随后进行了全分支独立审查。已登记的阻塞发现均已关闭；此结论只覆盖规定的语言 profile 和验收范围。

| 范围 | 审查中修正的主要问题 | 保留的实现与回归记录 |
| --- | --- | --- |
| 公共 Host | 严格协议、图校验、退出与后代所有权；提前 EOF 的诊断遗漏并入下方 M1 | `src-tauri/src/structure_review/native_analyzer/tests/` |
| Go | 条件表达式、同包及导入上下文的目标覆盖、workspace 路由、泛型与类型关系、排除测试文件的时机 | [Go 实现记录](go-native-implementation.md)、`native-helpers/go/review_test.go` |
| Java | 配置间绑定隔离、重复声明、真实 Maven 父配置、同源码多配置并集、来源与内容归属 | [Java 实现记录](java-native-implementation.md)、`native-helpers/java/tests/test_helper.py` |
| C# | Debug/Release 条件、项目引用配置传播、嵌套泛型身份、接口关系与目标引用程序集隔离 | [C# 实现记录](csharp-native-implementation.md)、`native-helpers/csharp/tests/test_helper.py` |
| Clang | 末尾 token、优化条件分支、extern C 内容归属、多 declarator 与成员指针范围 | [Clang 实现记录](clang-native-implementation.md)、`native-helpers/clang/tests/test_helper.py` |
| 应用与支持包 | Windows runner 与固定工具目录匹配、CI 路径过滤、未解析关系及结构/关系差异的公共入口断言 | [支持包记录](native-pack-integration.md)、[真实图记录](native-helper-conformance.md) |
| 共享 Windows 回收 | 成员通知排空、准确句柄等待、累计成员覆盖、失败仍回收直接 Child，以及 LSP/Python 接入 | [双平台与共享回收记录](native-crossplatform-validation.md) |

语言分项修复复核不会代替公共入口或 Windows 原生运行；这些验收后来在下述生产提交上完成。

## 2026-10-04 目标版本清理的独立审查

固定比较点为 `c379e0c9fbb47e4551e2e21b7d90a25379d93f24`，范围为 Java/Go 本机目标发现、
Clang 逐 TU 标准、对应打包和验收；既有 TS/Python 策略不在本轮范围。

- Standards：0项硬规范问题；1项低优先级重复校验建议已处理，将两处包语言/SDK兼容判断收拢为 `module.validateSDKVersion`。未要求额外通用发现框架或解释器抽象。
- Spec：发现 Go 将明确语言版本错误当作普通未知关系；固定编译器的公开诊断被提升为失败，并补回 Go1.23 默认关闭泛型别名实验的兼容约束。缺少业务依赖/API仍保持未知。
- Spec 修复复核：仓内包全部检查函数体后，旧缓存无法区分是否包含测试，可能漏掉随后选中的测试对象。增加独立 `TestsIncluded` 状态，重新加载所需测试；四个顺序/错误回归修复前均失败，修复后通过。
- 最后定点复核通过，上述两项 Spec 发现关闭；完整Go36项与22个子用例通过，无跳过。复核检查代码和日志，未重复构建测试。
- 实际安装另发现 Go 安装器仍要求包内SDK，两条旧入口要求已删除；Java发现也与helper支持的经典JDK目录布局对齐。四包生产安装与最新八图均通过。

本地 Rust764项、脚本24项及真实 helper 结果见[目标版本记录](native-target-version-discovery.md)。
Actions结果单独记录，不用静态审查或本地成功代替另一操作系统的验证。

## 2026-10-04 简化修改的独立审查

本轮固定比较点为 `21aed5b`，按用户新要求审查超时、平台指纹删除、本机SDK/引用发现、共享
进程接口与夹具；没有重新宣称覆盖整条历史分支。Standards与Spec两轴独立检查后完成修复复核：

- Standards：没有新增代码规范问题或阻塞性复杂度问题；指出并修正ADR与语言规格中的旧目标/摘要政策。
- Spec：Java与Clang在删除fingerprint时残留缺少key参数的调用，造成编译失败；移除残余条件，保留版本校验，四包重新构建通过。
- Spec：协议中仍写旧LSP/Python未共享的段落已同步为`ProcessTree`，并保留清理失败语义。
- C#疑点通过真实helper复现：C#9+ImplicitUsings曾成功返回；现在在生成global using之前明确拒绝，SDK项目/source-set各测9/10边界，具体错误与成功路径均通过。
- 后续Windows夹具修正经过定点复核：明确已退出PID离开当前列表的前提，准入测试计入系统控制台成员；累计覆盖、准确Job归属、1816错误及清理返回时的全部句柄退出断言均保留，未发现新问题。

修复复核无新发现，以上项均关闭。完整本地结果与本轮Actions证据见跨平台记录；
审查通过不替代实际平台运行。

本轮代码提交`1679e22`的[完整Actions37175433090](https://github.com/XLCYun/narrant/actions/runs/37175433090)
四组全部成功：macOS/Windows四语言原生链路与既有Python链路均通过，Windows共享进程11项及
LSP13项通过。独立轻量复验另有11项共享与14项LSP/URI测试通过，不与完整矩阵重复相加。
没有剩余审查发现；自动发现本机目标仍受编译器、标准声明及静态项目模型的既有能力边界约束。

## 全分支审查发现及关闭

全分支审查检查 `6761aca..5e743c8`，发现 I1–I8 和 M1/M2；修复复核检查
`c6bbb65..ea2f28b`，另外发现 N1/N2。N1 在 `e1b1586` 定点复核关闭，N2 在 `42e5a26`
定点复核关闭。最后一轮是测试驱动修复，不改变当时的生产 helper 构件。

| 编号 | 原问题 | 修复及行为回归 |
| --- | --- | --- |
| I1 | Go 丢弃空白标识符初始化内容和调用 | 为实际执行的初始化建立稳定内容所有者；检查参数变化、调用目标与共享表达式只归属一次 |
| I2 | Go 将必要仓库上下文的配置错误降为缺少绑定 | 传播必要源码读取、语法及配置错误；验证导入闭包中的未知标签和缺失目标 |
| I3 | Go 泛型 receiver 被类型参数冒充所有者 | 从原生 receiver 语法取得基类型；值与指针 receiver 均指向真实类型 |
| I4 | Java 在 Host 输入断开后仍继续分析并成功返回 | EOF 触发取消；真实入口验证断开后不返回成功图 |
| I5 | Java 接受但忽略 Maven 父项目的源码根 | 静态模型继承受支持的主/测试源码根；验证默认目录不再被误接纳 |
| I6 | Java 泛型参数改名改变方法持久身份 | 按词法作用域和位置归一化；验证重命名、嵌套与遮蔽 |
| I7 | Java record 父签名重复拥有组件内容 | 父签名采用有序子锚点；组件类型变化只改变组件自身内容 |
| I8 | C# 将接口继承标为实现 | 同时判断源声明和目标类型；分别断言接口继承及类/结构体实现 |
| M1 | Host 在 stdout 提前 EOF 时丢失 stderr 原因 | 有界排空输出并保留原错误；验证两种 stdout/stderr 到达顺序 |
| M2 | 支持包烟测 kill 后立即清理工作目录 | 等待 close 再结束，超时保留失败；验证退出与目录删除顺序 |
| N1 | Go 在排除导入包测试之前检查其构建标签 | 加载器先排除 `_test.go`；`TestImportedExcludedTests` 同时验证排除与明确包含场景 |
| N2 | Java 测试驱动在阻塞读取期间失去超时 | 覆盖完整交互的 watchdog；`DriverDeadlineTests` 验证 ready/completed 卡住时终止、回收及保留诊断 |

上述回归保留在 Go `review_test.go`、Java/C# 原生测试、Host 测试及
`scripts/tests/native-language-support-packs.test.mjs` 中。审查复核与运行测试是不同证据：
部分复核直接检查修复 diff 和既有测试日志，没有重复运行相同套件。

## 2026-10-03 共享回收修复与历史验收

后续 Windows 平台验收实际复现了旧共享 owner 提前返回的缺陷。生产修复将 LSP/Python 与
原生 helper 统一接入 `child_process/windows_job.rs`，保留准确句柄及完整性失败判据；
生产迁移和测试迁移均经过独立审查。早期“Windows 待验证”的报告不能作为最新状态。

该轮生产提交为 [`83d5642`](https://github.com/XLCYun/narrant/commit/83d5642c171415398e0b19c2338a46f08c468e79)。
[Actions 37128777728](https://github.com/XLCYun/narrant/actions/runs/37128777728) 的四组矩阵全部成功：

- macOS ARM64 / SDK 15.5 与 Windows x64：四包构建、ready、四语言原生套件、Host、factory、实际安装和八项公共图验收通过。
- Windows：共享 owner、Transport finish/Drop、LSP 正常退出/取消/超时/初始化失败及短命后代批次通过；准确句柄立即退出断言保留。
- Python：双平台分析、安装与 Review 生命周期通过；Windows 三项真实固定 ty 会话实际执行。
- 本地最终 Rust 全量 753 项通过，另有固定工具测试默认 ignored；实际工具执行以 Actions 记录为准。20 项受影响脚本测试通过。

精确测试数量、跳过原因、候选与旧实现对照、历史失败以及日志解释统一见
[跨平台验收记录](native-crossplatform-validation.md)，不在本文件复制完整时间线。
实际覆盖不等于支持任意 SDK、任意构建配置或全部语言扩展；支持包仍为 `published:false`。

## 记录保留方式

本文件及链接到的实现、验收记录替代本地 `.superpowers/sdd/0024b-native-analyzer-implementation-plan/`
中的分项审查和复审工作报告。原始临时 `review-*.diff` 可按对应提交从 Git 重建，清理时一并删除。
保留仓库中的行为回归测试和提交历史；不把本机 `/private/tmp` 路径当作 PR 审查者可访问的证据。
2026-10-04最初的`21aed5b`仅整理文档和本地工作记录；随后本轮简化修改与审查另记于上文。
未手动触发 quality-gate。

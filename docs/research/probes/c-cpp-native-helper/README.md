# C/C++ 原生分析配置并集探针

本探针验证 [ADR-0034](../../../adr/0034-use-native-analyzer-helpers-for-new-languages.md) 的单图并集是否有可用的 Clang 源码和绑定证据。它不是产品 helper，不生成 `CodeGraph`，也没有编译或测试 LibTooling。Clang CLI 的 JSON AST 仅作实验观察工具，不能直接作为正式 helper 协议。

## 复跑

需要机器已有 Python 3 和 Clang；不安装任何工具或依赖：

```sh
python3 docs/research/probes/c-cpp-native-helper/probe.py --clang /usr/bin/clang
```

默认只向 stdout 输出 JSON。需要另存报告时加 `--output <path>`；[已记录结果](observations-2026-10-02.json) 来自 2026-10-02 的 Apple Clang 21.0.0、macOS ARM64。复跑时工具版本或 JSON AST 格式变化导致断言失败应保留为差异，不应删除断言迎合结果。

脚本在 `/tmp` 的独立临时目录创建自身携带的源码夹具，并自动清理。每个 AST 调用都使用 `-fsyntax-only -nostdinc -pedantic-errors`；只运行受控系统 Clang，不链接或执行夹具程序，不运行仓库构建或脚本，不访问第三方依赖或 SDK。两个宏配置 `FAST=0`、`FAST=1` 是**明确提供的输入**，未验证自动发现或宏组合穷举。调用前后核对夹具文件名、内容摘要与目录，确认这些调用未更改夹具文件或生成输出；这不等于证明所有 Clang 功能在所有环境中都只读。

## 实测结论

同一组夹具分别以 `-x c -std=c17` 和 `-x c++ -std=c++20` 运行，每组在另一个临时根目录颠倒配置顺序重跑，以下归一化观察完全一致。

| 情况 | 真实 AST 证据 | 对单图并集的意义 |
| --- | --- | --- |
| `int run(void) { return PICK(); }`，`PICK` 分别展开为 `fast`、`safe` | `run` 的原始源码范围相同；两个 CallExpr 的 `referencedDecl.id` 分别指向有源码位置的 `fast`、`safe` FunctionDecl | 一个源码对象可以合并两条已解析调用关系；目标不是由名字猜测。 |
| `#if FAST` 与 `#else` 各声明一个 `mode`，返回值不同 | 每次只有一个 `mode` 声明进入 AST；两个声明在同一原文的第 4、8 行；`branch()` 的引用各自绑定本配置的声明 | 两个**不同源码位置的源声明**可以分别保留，同名不构成同一源对象。不能按名称或链接名覆盖其中一个。 |
| `RESULT value(void) { return 1; }`，`RESULT` 分别展开为 `int`、`double` | 同一 FunctionDecl 源码范围、同一原始文本；`qualType` 不同。源码范围起点的 expansionLoc 指向 `RESULT` 使用处，spellingLoc 则在对应 `#define` | 采用原始源码签名可避免把展开类型差异误作源码内容冲突；语义类型继续用于各配置绑定。不能把其中一个推导类型无条件宣称为该对象的唯一类型。 |
| `DECL item;`，`DECL` 分别展开为 `typedef int`、`int` | 同一声明源码位置和原始文本，AST kind 为 TypedefDecl / VarDecl | 如果 `item` 是承诺支持的独立对象，它的 Type / Value 类别冲突无法靠原始文本解决，按 Q5 整次失败；不能取第一份，也不能加配置后缀伪造两份源码。 |
| 两个条件分支分别打开 `struct Left {` / `struct Right {`，随后共用 `int slot;` 和 `};` | 同一 FieldDecl 源码位置和内容，实际父声明分别为 Left / Right | 如果字段独立建对象，同一 `slot` 在并集中有两个父对象，违反单父包含约束；原始文本无法消除此冲突，按 Q5 整次失败。 |
| 同一 `void *` 到 `int *` 隐式转换源码 | 严格 C17 exit 0，严格 C++20 exit 1 | 共用 Clang 核心仍需独立 C/C++ 模式和验收；不能用 C++ 成功情况推断 C 全部已支持。 |

这里的类型差异案例只有简单具名无重载函数。如果正式映射把推导类型写入 `signature`、扩展字段、对象身份或其它必须单值的图字段，就仍须改变映射或拒绝归并。**“原文相同”不是忽略所有语义冲突的通行证**；类别、所有权、重载身份、模板实例和对象拆分都需要独立判断。

本探针输出的是特定连续声明的原始范围候选，不是完整的 own-content 规范化器。AST 范围对于 `DECL item;` 不包含末尾分号；正式投影还需处理标点、注释、独立子对象剔除、宏定义/调用归属和非连续来源。示例函数没有独立子对象，因此足以验证“不要将宏展开结果代替用户源码”的局部事实。

## 尚未验证

- 字节偏移、行号只用于同一不可变原文内确认源片段。它们不是建议的生产 NodeId，更不能证明跨编辑身份稳定；相同相对文件、不同 checkout 的测试只排除了本探针结果受临时绝对根影响。
- 没有实现稳定源身份、USR 生成、声明/定义目标策略、完整六类映射、最终 Node / External / Unresolved 分类或 Rust 通用图验证。
- 没有验收真实 LibTooling `SourceManager` / `Lexer` / `PPCallbacks` 路径，JSON AST 的位置现象不能替代这些 API 的实际集成验证。
- 没有验收自动配置发现、编译数据库读取、跨 TU / 头文件组合、模板/重载、宏生成多个对象、target SDK、modules、缺依赖路径或 Windows。
- 没有产品性能、取消、崩溃与进程树回收验收；没有运行 Cargo 或 quality-gate。

下一步建议先在真实 LibTooling helper 中复刻这五类归并夹具，再扩充宏源码范围和跨编辑身份测试。已通过的简单例子支持继续原型工作，不构成完整 C/C++ 支持的验收结论。

---
status: accepted
date: 2026-09-29
---

# Python Code Graph

用户已确认 Q1–Q7，设计访谈已完成。本页是实现范围、技术决定和验收条件的来源；
规格接受不代表功能已经交付。研究依据与有限实测见
[Python 支持调研](../research/python-structure-analysis.md)。

## 产品目标

为现有 Structure Changes 增加 Python 支持，比较同一 Review Snapshot 两侧改动文件的
代码对象、自身内容和可确认关系，复用现有 Code Graph、差异计算、工作区和展示流程。
不因语言扩展改变 Review 或图的成员范围。

## 已确认的首版范围

- **Q1 — 文件：** 普通 `.py` 与 `.pyi` 项目，包括服务、库、CLI 和测试代码；
  Notebook 不进入首版。测试仍受现有 `include_tests` 选项控制，默认排除可明确识别的测试。
- **Q2 — 分析上下文：** 只使用本侧仓库源码和固定标准库类型声明，不读取用户已有解释器、
  虚拟环境及其第三方依赖，也不自动安装或执行被审查项目。
- **Q3 — 嵌套函数：** 函数体内的 def、lambda 和回调保留在最近外层 Callable 的内容中，
  内部调用关系归外层；独立显示不在任何函数体内的具名函数与类方法。
- **Q4 — 类字段：** 类体属性和可确认归属的显式实例属性均独立显示，实例属性挂在类上；
  不根据 `self` 字符串或动态 `setattr` 猜归属。
- **Q5 — 模块变量：** 普通顶层变量和常量合并入模块执行区域，保留初始化/赋值变化；
  显式类型别名仍为 Type。
- **Q6 — 局部类：** 函数体内的 class 及其字段、方法完整保留在外层 Callable；
  其内容修改与内部调用仍归外层，不再生成独立局部 Type 或成员节点。
- **Q7 — 技术与版本：** 采用 Ruff parser + 固定 ty 支持包；输入目标为 Python 3.7–3.14，
  3.7–3.9 的旧标准库与关系解析单列兼容验收。技术边界与交付门槛按本页执行。
- 第三方上下文缺失时，保留可以确认的仓库内关系，其余使用现有 Unresolved 表示。
  语言工具不可用、源码提取失败和协议/生命周期错误仍需明确失败，不得返回成功空图掩盖。
- 此范围不扩大图成员：未选中的仓库源码仍可提供语义上下文，其已解析目标属于 Repository
  External；标准库目标属于 Dependency External。

Q2 的取舍记入 [ADR-0033](../adr/0033-bound-python-analysis-to-repository-and-standard-library.md)。
它延续 [ADR-0032](../adr/0032-separate-language-support-packs-from-project-toolchains.md)
的工具包边界，不引入用户项目工具链管理。

## 继承的共用约束

遵守 [0018](0018-structure-review-code-graph.md)、[0020](0020-structure-changes-review-view.md)
和 [ADR-0031](../adr/0031-reuse-review-head-for-structure-analysis.md)：

- 两侧使用相同分析器版本和选项，在各自固定工作区内分析，不另复制 Python 源码或项目配置。
- 使用现有六类 Code Object 和语言原生 kind，不将 Python parser 的原始节点种类暴露为产品模型。
- 身份不包含绝对 checkout 路径、行号或临时服务 symbol 编号；不推断重命名或移动。
- 原始 source 用于展示；规范 signature/code 与关系分别参与比较。纯普通注释/排版变化不应造假差异。
- 对象图输出和 UI 显示范围分开；复用既有语言切换、目录分组和 External/Unresolved 显示规则。
- 任一语言的硬失败导致完整比较失败，并在释放工作区前结束相关工作和子进程。

## 技术决定

2026-10-11 按 [ADR-0035](../adr/0035-migrate-existing-analyzers-to-native-apis.md) 与
[0025](0025-migrate-existing-analyzers-to-native-helpers.md) 使用 `native-helpers/python`。
固定 ty 0.0.84 对应 Ruff revision `11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a`；
同一数据库提供 AST、原文与全部导航候选，helper 负责既有投影并经公共 Native Host 构图。
旧 Host Ruff parser、ty 服务器、LSP/setup/smoke 路由已经退役。无需用户 Python/Node 环境，
不自动切换其他引擎。1.1.0 未发布候选完成本地 macOS ARM64 实际安装/Review 验收；
最终 Windows x64 与集中双平台 CI 待完成。边界、历史断言和许可证来源限制见
[helper 说明](../../native-helpers/python/README.md) 与
[迁移证据](../research/existing-native-helper-migration-validation.md)。

当前没有未决产品问题。实现验证若推翻已确认范围或工具选择，应记录证据并重新打开对应决定，
不得用未验证的兼容性或默默放宽分析边界满足验收。

## 对象映射与自身内容

| Python 构造 | Code Object | 内容归属 |
| --- | --- | --- |
| 有保留对象/内容的选中文件 | Container / `file` | 仅组织文件；原文用于展示，不重复比较整个文件 |
| 函数体外的具名 class、显式类型别名 | Type / `class`、`type_alias` | 类头与有序类体；别名保留类型表达式。Protocol/Enum 的特征须有可信依据，不按短名称猜测 |
| 函数体外的 def、async def、方法 | Callable / `function`、`async_function`、`method` | 装饰器、参数默认值、注解等归签名，函数体归自身 code；内部 def/lambda/局部 class 按 Q3/Q6 保留原结构 |
| 可确认的 property 访问器 | Callable / `getter`、`setter`、`deleter` | 每个访问器独立，角色参与身份；不额外复制为同名 Value |
| 类体显式属性、可确认的实例属性 | Value / `field` | 直接包含于类；类体初始化器与实例方法中的运行时赋值分别处理，见下文 |
| 模块级语句、普通变量、import 及声明的有序投影 | ExecutionRegion / `module_body` | 每文件至多一个，保留控制结构、声明锚点与语句交错顺序；`.pyi` 记录声明内容，不声称存在运行时实现 |

空文件、只有普通注释的文件和被排除的测试文件不创建空容器。Python import 可能影响模块
执行，因此保留在模块内容中；import 不生成独立节点或 import 边。只有声明的 `.py` 文件
也保留模块投影，因为执行定义的先后仍可能重要。顶层 lambda 赋值保留在模块内容中，
首版不另推断函数值变量节点。不生成隐式构造函数、dataclass 生成方法或没有直接源码的对象。

显式类型别名包括 `type Name = ...` 和已确认 `typing.TypeAlias` 的注解声明；
普通 `Name = value` 不仅凭大写名称、表达式形状或推断类型自动升级为 Type。
直接类体的赋值/注解声明可形成字段；控制结构中的动态赋值仍留在类自身内容中。

### 保留执行顺序，隔离子对象修改

模块 ExecutionRegion 与 class Type 的 `code` 使用 AST 有序投影：普通语句完整保留，
独立建模的声明在原位置替换为稳定对象 ID 锚点；if/try/for/with 等控制结构不能被展平。
锚点是比较表示，不写回项目源码，也不提交给 LSP。

例如 `n = 1; def f(x=n): ...; n = 2` 投影为 `n = 1; <declare f>; n = 2`。
把 f 移到最后会改变模块内容。f 的默认值、装饰器和注解仍完整保存在 f 的签名中，
函数体保存在 f 的 code 中；不将这些子内容再次复制进模块或类。
类字段初始化器保留在字段 code，其位置由类体锚点记录。

- 修改独立方法的函数体，只改变该方法的自身内容，不改变类或模块的自身内容。
- 修改默认值、装饰器或基类表达式，改变所属声明的内容；其中的引用也归该声明。
- 交换类字段，或移动声明跨过普通语句，改变相应执行投影。
- 修改嵌套函数、lambda 或局部类，改变外层 Callable；其内部调用仍归外层。
- 多目标/解构赋值的完整表达式不能拆散或重复求值式建模；字段保留可确认声明信息，
  无法唯一归给某一字段的共享初始化表达式留在类体投影中。

这些规则使用已有 `code` 身份占位能力，不改变通用 `contains` 的无序集合语义。
规范化忽略普通注释与纯排版，保留 docstring、字符串/f-string、表达式与缩进形成的结构；
不通过正则删空白。`.pyi` 的省略实现属于声明信息，不伪造函数体；有意义的声明文档仍参与比较。

### 实例字段的证据与归属

首版确认直接实例方法接收者上的显式赋值/注解目标。接收者必须来自该类实例方法的参数绑定，
且没有被重新绑定、遮蔽或传入未知别名链；不能只检查名称是否为 `self`。
已确认 staticmethod/classmethod、未知装饰器改变方法身份、任意对象写入、嵌套闭包写入、
`setattr` 字符串和无法确认归属的描述符写入均不足以新建实例字段。

字段身份按类与属性名去重；类体注解和方法中的同名实例属性在可确认归属时合成一个 Value。
Value 保存名字、可确认的显式类型声明和类/实例来源信息，不能把推断出的类型当成显式声明。
`receiver.x = expression` 的完整赋值及其中调用仍属于方法 code，而非字段的初始化器。
同一字段的第二处写入不再新建字段；修改其 RHS 只修改方法内容。
重复类体声明的初始化器按原顺序保留，并记录各声明片段的执行锚点。

### 身份、重绑定与 overload

身份由语言、仓库相对路径、词法 owner、kind/角色与声明名称组成。同一 owner 下同名普通
重复声明保留不同对象，用同名声明序号区分；插入同名声明可能使后续序号变化，不做猜测匹配。
选择文件范围或 checkout 绝对路径变化不得影响已有身份。

只有确认来自标准库的 `typing.overload`，且属于同一词法位置的合法声明组时，才将有序
overload 签名与对应实现合成一个 Callable；每个源码片段保留原始范围与锚点。
`.pyi` 中合法的纯签名组没有实现。普通重复 def、条件分支中的同名定义、被遮蔽的同名
decorator 不按 overload 合并。property 同样验证 decorator 绑定及访问器链，未知时保留
普通声明，不能只凭 `@property` 或 `@name.setter` 文本决定角色。

`.pyi` 与 `.py` 各自保留源码所有权与 ID，不合并跨文件节点。语义目标只有在证据明确时
映射到相应源声明；不得无条件“优先实现文件”或“优先第一项”。

## 关系与测试范围

沿用调用、函数值引用、继承、成员归属和可确认字段类型关系；不新增 reads/writes、import
依赖图或运行时追踪。语法层收集实际出现的引用位置，LSP 提供目标证据，不能只依赖
call hierarchy 的输出。按上文的自身内容归属分配引用，避免父子重复收集。

definition 是源声明导航证据，不是所有可能运行时实现的枚举。过滤 import 自身锚点，
校验目标声明类别、别名链、坐标、所归项目及真实路径；明确唯一且合格的目标才建立确定关系。
普通赋值别名只在单一绑定、无重绑定/未知条件等反证时继续追踪。
已知条件分支歧义、互相冲突的项目结果、未知装饰器或动态属性使用 Unresolved；
不能因为工具仅返回一个目标就覆盖已知歧义，也不能用 typeDefinition 猜运行时调用目标。
类调用可指向可确认的源类声明，不制造不存在的构造方法。

目标在本侧选中文件的保留图中为 Node；已解析到其他仓库文件为 Repository External；
标准库为 Dependency External。被测试选项排除的对象不重新进入图或通过边显露。
缺第三方依赖只影响无法确认的关系，不抹去已提取对象。复用当前 UI 隐藏 External/Unresolved
的规则；本方案不新增“完整调用图”承诺或另一套可视化。

测试过滤采用确定的文件约定：仓库相对路径含 `tests` 目录段，或 basename 为 `test_*.py`、
`*_test.py`、`conftest.py`；相应 `.pyi` 同样识别。仅依赖 checkout 外祖先目录名称不得影响结果。
`include_tests=false` 时不输出这些文件的节点/关系；开启后按原改动文件范围恢复。
普通生产文件中仅有 test 命名函数或 pytest 调用不整文件排除。未知和共享代码保留；
不运行 pytest/unittest 做发现，也不读取测试框架安装环境。

## 历史工具与现行项目配置

下面的两层编译/LSP 描述记录迁移前版本背景；现行 helper 的 Ruff/ty 依赖固定在同一
revision，构建工具链为 Rust 1.98.0，AST/导航共用不可变快照。输入版本、编码、
项目归属、PEP 723 和测试开关规则仍保持；历史服务验收不是本轮平台验收。

- 语法层固定 Ruff `0.16.9` 对应的 parser/AST/codegen `0.0.15` 同批兼容依赖，由应用编译。
  不把 Ruff CLI 当语义工具，不依赖内部 crates 的跨版本稳定性。该批 MSRV 为 Rust 1.96；
  本机 Rust 1.97.1 已完成锁文件解析、编译和提取验证。
- 语义层固定 ty `0.0.84` 原生服务，由独立 Python Language Support Pack 提供二进制、
  固定标准库声明、许可证和启动所需控制文件。记录上游摘要与独立包版本；不跟随 latest。
  Pyright `1.1.414` 仅作为备选设计，不在失败时自动混用另一套语义。
- 输入目标范围为 Python 3.7–3.14；两层显式使用同一实际目标，不为了达到 ty 的
  正式类型检查支持线而把 3.8 项目改按 3.10 分析。`.pyi` 使用 parser Stub 模式。
  同时检查普通语法错误与 `unsupported_syntax_errors()`，不把部分恢复 AST 当完整成功。
- 3.10–3.14 作为两工具正式支持范围的主要验收；3.7–3.9 另做兼容验收。ty 支持选择旧目标，
  但 bundled 标准库声明不完整，可能影响类型推断与成员定位；这不是不能提取类/函数的证明。
  不能仅因目标低于 3.10 拒绝分析，也不能声称旧版本关系质量已与新版本等价。
  缺少合格目标证据仍为 Unresolved；协议/解析错误仍为硬失败，不以兼容支持之名吞错。
- 首版源码编码为 UTF-8（含合法 UTF-8 BOM/编码声明）；其他编码明确报告不支持。
  规范化与 LSP 始终基于同一份文本；不得忽略非 UTF-8 编码声明后按错误字符集解析。
- 显式目标读取仓库中对应项目的 ty 配置；同目录 `ty.toml` 优先于 pyproject 中的 ty 段。
  无显式目标时，从适用的 `requires-python` 选择最低受支持 minor，无约束固定 3.14。
  PEP 723 的脚本约束优先于项目的兼容范围；其依赖声明不触发安装。显式目标若与兼容范围
  冲突应报配置错误，不使用本机解释器猜测。
- 按版本范围判断是否存在可用 patch，不把 minor 简化为 `.0`：`>=3.8` 选 3.8，
  `>=3.8,!=3.8.*` 选 3.9，`>=3.10.5` 仍可选 3.10。显式指定不支持版本，
  或兼容范围与受支持集合无交集时明确失败。固定分析目标不是对旧 Python 运行兼容性的认证。
- 固定 ty `python-platform="all"`，避免默认按宿主操作系统收窄 `sys.platform` 分支；
  由此产生的多目标继续按歧义规则处理。此设置已纳入真实服务多目标反例；不能据此推断所有运行时分支等价。

3.10 是 ty 的正式类型检查支持下限，不作为本产品的输入下限。
Ruff 固定版本最低支持 3.7，最新稳定目标 3.14，3.15 为 preview；ty 明确允许 3.7–3.9，
只是其旧标准库声明未完整覆盖。所有 Python 3 不能直接承诺：3.0–3.6 有旧语法兼容差异，
3.15 及以后也不能由当前固定工具自动获得完整支持。
[Ruff 版本源码](https://github.com/astral-sh/ruff/blob/0.16.9/crates/ruff_python_ast/src/python_version.rs)、
[ty 固定版本说明](https://github.com/astral-sh/ty/blob/0.0.84/docs/python-version.md)。
3.7–3.9 已完成类/函数提取、仓库调用、`len` 和字符串成员的真实 LSP 兼容回归；
具体证据与剩余发布门槛见[实现验收](../research/python-implementation-validation.md)。

项目归属按选中文件就近的仓库内 `ty.toml` / 含项目或 ty 设置的 `pyproject.toml` 确定，
不得越过仓库边界读取父目录配置。每个项目单独归一化合法的源码根、src layout、目标版本和
允许的额外源码路径；不把 monorepo 所有子项目合成一个无差别搜索路径。
文件不属于已识别项目时，以本侧仓库根为项目并使用固定默认值。
版本或项目配置不同的文件使用独立语义会话；同一侧可复用相同配置会话，两侧不能共享可变状态。
路径规则不扩大图成员；选中节点仍由 Change Comparison 确定。

## 仓库与标准库边界

现行 `PythonProject` 将已校验仓库源码、目录及固定内嵌 typeshed 冻结到内存 System，
该 System 没有写入、环境发现、命令执行、配置目录或缓存能力。project/版本/源码根
显式传入，空 Python prefix 和 `UseUv::Off` 阻止解释器/uv 自动发现；可达 symlink/junction
仍校验真实来源。加载后 AST、source 与导航读同一快照，不把缺依赖变成伪绑定。
下列原服务启动政策保留作历史边界说明；不再通过服务器控制文件实施：

1. 工具包自带固定空控制配置，通过 `configurationFile` 指定；仓库配置只在内存读取、
   归一化并生成允许的 inline configuration，不复制清洗后的项目配置树。
2. 设置 `untrustedWorkspace=true`、关闭 uv 集成，禁止项目命令、解释器探测和依赖安装。
   显式指向实际存在的受控空 Python prefix；不存在的哨兵路径不能防止自动环境回退。
3. 将用户配置目录与缓存定向到受控位置，清除影响 Python/Conda/uv/ty 环境发现的继承变量。
   stub 缓存属于工具数据，使用固定身份；Windows 的实际环境/目录处理必须原生验证。
4. 项目路径在内存按原配置所在目录解析成绝对路径后再传递；不误用服务进程 CWD 解释相对配置。
   只提供本侧仓库内源码根与固定 stdlib。用户 venv/site-packages、外部额外路径、外部配置
   继承及自定义 typeshed 不加入分析上下文；其缺失关系按 Unresolved 处理。
5. 在启动服务前检查可达源码根和配置路径的真实位置，包括 symlink、Windows junction 等。
   仓库内部链接可保留；可达链接逃逸仓库、无法验证的路径或无法建立约定边界时明确失败。
   返回目标仍做 canonical path 与合格声明校验，不能只以 URI 字符串前缀或响应非空判定成功。

这是既有固定 Review Workspace 一致性条件下的分析上下文约束，不宣称提供对抗并发恶意
文件系统修改的 OS 沙箱。若原生 ty 无法通过这些反例，不得放宽 Q2 后继续发布；应补受限
文件系统接入或重新评估 Pyright 固定宿主，并更新本规格的技术决定。

## 已验证内容与发布前门槛

本轮完成官方固定源码核查，以及真实 LSP 基础探针和 ty 边界探针。
[基础证据](../research/probes/python-lsp/README.md)包括两个服务各 14 个 definition 查询；
[边界证据](../research/probes/python-lsp/boundary/README.md)包括 ty 的 8 个会话、64 个查询。
仓库/标准库导航可用；污染配置与虚拟环境可被受控组合排除；外向 symlink 的反例已实际复现。
所有归档探针会话正常退出，未执行被审查 fixture 代码。

这些前期探针本身没有验证完整 Code Graph 和产品集成。实现阶段的对象/关系、边界、
多项目、原生进程、应用和界面验证另见[实现验收](../research/python-implementation-validation.md)。ty/Pyright 第一轮的有效目标版本不同，
不能据此给准确率排名。资料记录工具支持某 LSP 方法也不等于已经测试过该方法。

实施验收至少包括以下成对样例及失败路径：

| 维度 | 必须验证 |
| --- | --- |
| 对象与内容 | 普通/async 方法、property 各访问器、显式别名、真实/伪 overload；普通重绑定不合并；嵌套 def/lambda/局部 class 按已决粒度归属 |
| 差异投影 | 方法体变化不污染父；默认值改变子签名；声明跨赋值移动、字段交换及条件块内声明改变正确 owner；普通注释/排版无差异，docstring/字符串修改有差异 |
| 字段 | 不名为 self 的接收者仍可确认；同字段多处赋值不重复；RHS 变化只改方法；重绑定 self、staticmethod、动态 setattr 与未知装饰器不能伪造字段 |
| 关系 | 相对导入/src layout、跨文件方法、类调用、可信别名、继承/类型关系、`.pyi/.py` 对应；缺第三方、条件别名、重复赋值、动态成员均有反例；import 自身位置不算已解析声明 |
| 身份与坐标 | before/after 和不同 checkout 身份一致；新增/删除/重命名、重复同名声明、UTF-8/UTF-16、中文/非 BMP、LF/CRLF、原生 Python 换行与编码规则 |
| 配置与边界 | 3.7–3.14 各 minor 的语法/节点提取，3.7–3.9 旧标准库与成员定位兼容样例；范围交集/patch 排除/PEP 723；多项目同名模块不串；固定 all 平台；毒化仓库/父目录/用户配置与环境变量、venv、外向链接、stub 缓存失败 |
| 支持包与生命周期 | macOS ARM64 与 Windows x64 原生启动、LSP 查询和退出；摘要/缺文件/不可执行；安装重试、删除和租约保护；超时/取消/崩溃后无遗留进程，结束后才能释放工作区 |
| 应用与展示 | Rust/TS/Python 混合两侧合并、仅删除 Python 文件、测试开关；错误不可变成功空图；Python 包名/缺包恢复、`.pyi` 编辑器、宽窄窗口与长名称/弹层/关系状态 |

只对支持的版本、编码与对象范围内的合法输入承诺完整提取；语法/配置/协议的硬失败必须可解释，缺第三方依赖
不被升级为整个图失败。避免对语义查询失败、超时或坐标错误一律返回 Unresolved。

## 历史接入与交付顺序（迁移前）

1. **语法与内容闭环：** 新建 `structure_review/python_analyzer`，实现现有 LanguageAnalyzer；
   固定 Ruff 依赖，先验证版本、字节范围、对象身份、内容投影与公共 Code Graph 校验。
2. **受控语义闭环：** 复用 LSP/SourceText，实现项目配置归一化、受控启动、引用校验与回收；
   以上负例和真实服务测试是进入产品集成的门槛。
3. **应用和支持包：** 扩充 `LanguageAnalyzers`、文件分组、两侧合并与 bootstrap；
   扩充 PackId、manifest、打包/发布脚本、必需入口、历史版本扫描、前端包类型/错误白名单。
   Ruff 属应用编译依赖，ty 属独立支持包；复用既有按需准备与工作区/支持包租约生命周期。
4. **验收与发布：** 补现有图视图和 `.pyi` 编辑器映射，两平台验证后再发布固定支持包；
   支持包仅 `--version` 成功不足以验收，需要 LSP 初始化、查询、退出。性能记录冷/热会话耗时、
   大文件/多项目查询量、缓存命中与取消延迟，超出既有生命周期限制必须解决后再交付。

实现票进入 GitHub Issues 并链接本规格。当前分支已实现 Python 分析器、独立支持包、
应用集成和 UI 映射；原生两平台检查及维护者审阅通过后才能合入和发布。无需新增通用图类别或插件注册框架。

| 实施票 | 范围 | 前置实施票 |
| --- | --- | --- |
| [#189](https://github.com/XLCYun/narrant/issues/189) | Ruff 语法阶段、代码对象、稳定身份与有序自身内容 | 无 |
| [#190](https://github.com/XLCYun/narrant/issues/190) | 受控 ty 会话、项目配置、关系校验与兼容验证 | #189 |
| [#191](https://github.com/XLCYun/narrant/issues/191) | Python 支持包、两平台工具入口与包生命周期 | #190 |
| [#192](https://github.com/XLCYun/narrant/issues/192) | 两侧应用接入、结构视图与端到端验收 | #189、#190、#191 |

规格与研究证据由[PR #188](https://github.com/XLCYun/narrant/pull/188)交付。
每张实施票仅跟踪自身范围、验收与依赖，完整产品要求以本页为准。

实现时 Rust 类型/方法使用中文注释，测试置于模块 `tests/`；Cargo 统一通过
`scripts/cargo-agent.sh`，优先聚焦验证，每 Issue 完成前一次全量测试。
前端仍需类型检查、Lint、构建及应用内浏览器/Preview Harness 验收，不手动触发 quality-gate。

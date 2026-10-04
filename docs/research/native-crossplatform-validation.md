# 原生 helper 的双平台验收入口

状态：2026-10-04 目标版本清理已通过本地验收、独立审查与完整 Actions。代码提交 `1f035bf`
在 macOS ARM64 / SDK15.5 与 Windows x64 的四组 Language support packs 验收全部成功。
共享 LSP/Python 与 native 已统一使用 `child_process::ProcessTree`，测试直接覆盖生产模块；
四包、四helper、Host、本机目标发现、实际安装及八图验收均通过。下方保留 `83d5642` 等历史基线。
交付位于[草稿 PR #200](https://github.com/XLCYun/narrant/pull/200)，未合并或发布支持包。
本记录覆盖指定语言支持与生命周期矩阵；未手动触发 quality-gate，不承诺更广 SDK 或版本。
审查覆盖、主要发现及关闭情况见[原生语言分析器审查记录](native-analyzer-review.md)。

## 2026-10-04 Java/Go目标发现与Clang逐TU标准

在上一轮本机SDK和共享进程接口的基础上，删除Java17、Go1.23及C17/C++20的产品门禁；
Java/Go从本机匹配项目目标，支持包不再附带额外目标库。Clang按CDB的显式标准分析稳定模式与GNU方言。
固定工具与CI样本保留。Go语言诊断降级及测试缓存漏选均在独立审查中复现、修复并完成复核。

代码 `1f035bf083d9192b218a1c1643eba988e0e59cbb` 的
[完整运行37187603144](https://github.com/XLCYun/narrant/actions/runs/37187603144)四组全部成功：
两平台Go套件、Java35、C#38、Clang35（各1项对端SDK不适用）、目标发现10、真实安装1和公共图8通过；
Windows共享进程11、LSP13、Host16与Python23/3/4通过，macOS共享3项/12场景、Host17和Python35/3/4通过。
本地Rust全量764、脚本24及Go36项通过，最新真实八图另显式执行。
详见[目标版本选择规则和完整矩阵](native-target-version-discovery.md)；下节保留此前提交的独立证据。
本轮通过PR更新自动触发，未手动触发quality-gate；最终仅文档提交使用 `[skip ci]`。

## 2026-10-04 共享进程接口与本机目标发现

相对 `21aed5b`，LSP/native 两套守卫合为 `child_process::ProcessTree`，Windows Job 生产算法
保持不变；Unix 平台快照/终止/等待与测试夹具集中维护。本轮代码、测试和配置净减902行
（不含文档），一次性调查程序已删除，1816限制与准确句柄退出断言保留在生产模块回归中。
三个清理入口各覆盖父存活/已退出及后代管道继承与否，共12场景；Windows另保留平台错误回归。

平台协议删除未实际比对的 fingerprint；支持包完整性校验不变。C# 从本机标准引用目录匹配
现代 TFM，并选择该 TFM 的最高稳定补丁；不再打包目标引用。C/C++ 发现本机系统 SDK，不再
依赖版本白名单。SDK 路径和实际声明仍须有效；本轮没有自动下载用户 SDK 或执行项目构建。

本地 macOS ARM64 / SDK27.0 验证：

- 四包重建及两次 ready 烟测均通过；Go 套件通过，Java25、C#38、Clang25通过，Clang的Windows SDK项不适用。
- Rust Host17、目标发现6、真实四包安装1（含同一运行时切换net8/net9/net10）、公共图8通过，无工具缺失跳过。
- 共享 Unix 12场景通过；Rust全量760通过、0失败，82项默认ignored中含1个进程夹具，另外8项真实图已显式运行。
- 43项脚本测试通过，2项仅适用于Windows的脚本用例跳过；Windows同源测试库跨目标check通过。
- 首次整组Clang测试因本机未设置已有CMake路径失败；补充`NARRANT_CMAKE`后完整组25通过/1不适用，没有代码豁免。

Windows 夹具复验修正了两项测试前提，未改动生产 WindowsJob 算法：

- Job 的当前 PID 列表可能暂时包含刚退出的对象；测试先建立所需状态，再验证已取出或排队中的通知不会丢失，不通过补采样账本绕过被测路径。
- 真实 runner 的父子进程还可能带有系统 `conhost.exe` 成员；准入测试不再假定总数恰为2，而是保存实际成员的准确句柄，核对累计覆盖和 Job 归属，严格要求新建失败1816，清理返回后立即检查每个句柄。

`2d2be17` 的[首轮完整运行37174259492](https://github.com/XLCYun/narrant/actions/runs/37174259492)
有三组成功，Windows原生组上述两项夹具失败。`fb7afd2` 的
[轻量运行37174825822](https://github.com/XLCYun/narrant/actions/runs/37174825822)确认实际多出的成员为系统控制台进程，
其余共享10项及LSP14项通过。`1679e22` 的
[轻量复验37175287661](https://github.com/XLCYun/narrant/actions/runs/37175287661)全部通过：共享11项、LSP14项，
另1个ignored函数仅用于子进程夹具；两类短命批次各创建256个Node子进程，累计账本与准确句柄断言保留。

最终[完整矩阵37175433090](https://github.com/XLCYun/narrant/actions/runs/37175433090)
验证代码提交`1679e220f4d6f64a72ec0cab396eef205ee83056`，四组全部成功：

| 验收组 | Windows x64 | macOS ARM64 / SDK15.5 |
| --- | --- | --- |
| 四包构建、ready与四语言原生套件 | Go通过；Java25、C#38、Clang25通过，1个macOS专用SDK测试不适用 | Go通过；Java25、C#38、Clang25通过，1个Windows专用SDK测试不适用 |
| 共享进程模块 | 11通过，含12场景矩阵、准入及通知回归；1个ignored函数仅作子进程夹具 | 3通过，覆盖12场景；1个ignored函数仅作子进程夹具 |
| LSP生命周期、短命批次与错误兜底 | 13通过，0ignored | 本矩阵未单独选择LSP测试组 |
| 原生Host | 16通过 | 17通过，包含Unix专用符号链接测试 |
| 目标发现与factory | 6通过 | 6通过 |
| 真实四包生产安装 | 1通过，133.54秒 | 1通过，83.96秒 |
| 公共CodeGraph/diff | 8通过，0ignored，28.52秒 | 8通过，0ignored，15.86秒 |
| Python分析器/安装/Review生命周期 | 23/3/4通过，全部0ignored | 35/3/4通过，全部0ignored |

Windows完整应用库与前置轻量库都执行了共享模块的11项回归；两次不是不同用例数量。
LSP串行和并发4两组各创建256个短命Node子进程，账本最终各保留515个准确对象，与Job累计一致；
清理约333.57ms和338.97ms。安装路径在同一个私有C#运行时下切换本机net8/net9/net10引用，
并验证旧目标不会误用新API。平台SDK样本仍为CI工具数据，不再是产品版本白名单。

本轮完整验证使用`validation_scope=full`、`publish=false`，独立轻量任务及发布任务按配置跳过；
没有手动触发quality-gate。后续纯文档提交使用`[skip ci]`，不把文档SHA记为新的代码实测。

## 2026-10-03 共享生产 owner 接入及完整验收：83d5642

进程所有者移到 `child_process/windows_job.rs`，LSP Transport 与 native ProcessTree
直接调用同一个 WindowsJob。迁移与 `b41e68a` 对照只有归属、类型和测试路径变化，清理算法保持一致。
删除旧共享Job实现及单元测试owner选择器，仅保留准确root观察、发送后取消和故障注入。
观察失败发生在真实ServerProcess守卫建立之后，错误路径仍由该守卫执行完整回收。

所有原始场景继续覆盖：共享owner6项、Transport2项、LSP13项、ProcessTree1项、URI1项；
轻量库可执行23个测试，另2个ignored子进程夹具；本次生产接入采用下述完整验收，未重复运行轻量组。
Python保留3个真实固定ty测试，
从5项减少仅因删除两组重复owner对照。生产与测试不会再走不同的清理实现。
本地库检查、Windows目标编译、20项脚本测试和独立生产/测试迁移审查通过。
本地Rust全量753通过、0失败、81项固定工具测试ignored，另8项helper集成测试默认ignored；
实际固定工具验证以下方Actions结果为准。

实际生产代码提交 `83d5642c171415398e0b19c2338a46f08c468e79`，
[完整运行 37128777728](https://github.com/XLCYun/narrant/actions/runs/37128777728) 四组全部成功：

| 验收组 | Windows x64 | macOS ARM64 / SDK15.5 |
| --- | --- | --- |
| 四包构建、ready与四语言原生套件 | Go通过；Java25、C#29、Clang25通过，1个macOS专用SDK测试跳过 | Go通过；Java25、C#29、Clang25通过，1个Windows专用SDK测试跳过 |
| 共享WindowsJob | 6通过，1个ignored函数仅作子进程夹具 | 不适用 |
| Transport finish / Drop | 2通过，原准确句柄Wait(0)断言保留 | 不适用 |
| LSP生命周期、短命批次与错误兜底 | 13通过，0ignored | 不适用 |
| 原生Host（含ProcessTree错误兜底） | 17通过，0ignored | 17通过，0ignored |
| factory | 2通过 | 2通过 |
| 真实四包生产安装 | 1通过，177.34秒 | 1通过，87.46秒 |
| 公共CodeGraph/diff | 8通过，0ignored，27.89秒 | 8通过，0ignored，13.86秒 |
| Python分析器/安装/Review生命周期 | 23/3/4通过，全部0ignored | 35/3/4通过，全部0ignored |

Windows LSP串行与并发4各8批、共256个短命Node子进程，最终保留515个准确对象与Job累计一致，
正常清理分别331.00ms和337.20ms。原先失败的正常退出、取消、超时、初始化EOF及Transport
finish/Drop均通过；缺失历史负例继续报错，未删除完成断言或将覆盖不足改判成功。
Windows Python的三个固定ty用例全部实际执行：正常关闭19.43ms、请求发送后取消与清理354.04ms、
公共调用图成功；准确根句柄完成、目录释放和仓库源码不变均保持断言。

本轮仅运行一次2分11秒的轻量候选对照和一次完整矩阵；后续验收文档提交使用`[skip ci]`避免重打支持包。
完整运行验证的是上述生产代码SHA，后续纯文档提交不另记为代码实测。
累计句柄账本有明确上限；无法确认覆盖仍保守失败。最终Drop监听线程join不承诺硬性两秒总时限。

## 2026-10-03 预算恢复后的轻量 Windows 验证

`Language support packs` 的手动 `validation_scope=windows-lifecycle` 只运行一个
Windows 2022 任务，使用固定 Node 24.18.0 和独立小型 Rust crate；不下载四语言工具链或编译 Tauri。
该 crate 通过源文件路径直接编译原进程所有者、LSP、ProcessTree 及其测试，没有复制清理算法。
原始 Win32 探针与全部库测试串行执行，前组失败不屏蔽后组，任何失败仍令整体失败。
普通 PR 与手动 `full` 保留原完整矩阵，轻量模式不能发布支持包。

本地复核进一步修正 native 候选的两个清理顺序问题：终止后继续消费成员通知直到累计覆盖，
已经取出的通知不得因为 stop 信号被丢弃；停止监听报错后仍等待已持有的准确对象句柄。
累计覆盖不足、监听错误与超时仍保留为失败，不放宽完成判据。新增三个确定性同步回归，
连同原始缺失历史负例一起纳入轻量验证；共享产品 owner 尚未迁移。

`b41e68a` 的[运行 37128210506](https://github.com/XLCYun/narrant/actions/runs/37128210506)
已在真实 Windows 2022 执行，单任务用时 2 分 11 秒。原始 flat/nested 诊断、三种产品清理场景
及1816门禁全部通过；库测试26通过、6失败，另2个ignored函数仅供子进程夹具执行。
六个失败全部来自旧共享实现，清理返回后准确后代 HANDLE 仍为 WAIT_TIMEOUT=258：
取消/超时的父先退出、初始化EOF、正常父先退出，以及Transport显式finish和Drop。
候选13项LSP全部通过；native6项全部通过，含新增3个同步回归与历史覆盖缺口粘性负例。

两种短命批次均实际创建256个Node子进程，串行与并发4的账本最终都是515个准确对象，
与Job累计总数一致，清理约331ms。有限对照中普通2次spawn令累计增加4、detached2次增加2；
计数不能直接按业务spawn次数推算。raw无额外控制台的flat/nested夹具维持外层累计2，
内层关联和同Job重复关联没有令外层增加；不支持“嵌套关联导致此前翻倍”的判断。
额外成员的具体映像仍未在该轮逐个识别，不将其全部断言为某个进程名。

这一轮提供共享接入前的兼容证据，同时明确保留旧实现红灯。产品默认owner接入后仍须重新验收；
不能以测试候选成功替代生产调用链成功。本地20项脚本测试和Windows-target编译也已通过。

## 2026-10-03 共享路径后续修复：c89643e 至 ebabed5

`c89643e` 增加仅 Windows 单元测试启用的 owner 对照入口，通过真实 `LspClient`、
`Transport` 和固定 ty `Session` 验证既有共享 owner 与 native 候选 owner。
产品构建继续使用既有共享 Job 算法；测试入口不构成产品接入。

`5d66c8c` 修正一个已由 Windows 故障注入复现的错误顺序：
`ServerProcess::reap_members` 保存 Job 清理错误，始终执行直接 `Child` 的 kill/wait，
随后返回原错误，且不把清理失败标为已回收。它不替代整棵树的完成验证。
这处修复尚未完成 Windows 故障注入回归：`5d66c8c` 的 native 任务在前置探针停止，
后续 `ebabed5` 的任务则未能启动。

| 提交与运行 | 实际结果与解释 |
| --- | --- |
| `c89643e`：[37118456774](https://github.com/XLCYun/narrant/actions/runs/37118456774) | Python 双平台与 native macOS 通过。Windows 新增5项真实Python会话全部通过；旧Job为1通过/2失败，LSP21为12通过/9失败。native Host、安装与8图均通过，整体仍失败。 |
| `5d66c8c`：[37121221638](https://github.com/XLCYun/narrant/actions/runs/37121221638) | Python Windows 25/3/4三组通过，新增5项会话均实际执行；两个macOS任务通过。native Windows 在新前置诊断探针失败，LSP22、Host、安装与8图没有执行，不能拼接上一轮结果声称本轮通过。 |
| `ebabed5`：[37121673745](https://github.com/XLCYun/narrant/actions/runs/37121673745) | 四个任务在任何步骤执行前均被 GitHub 计费检查拒绝，runner与steps为空；没有编译、探针或测试结果。官方错误指向付款失败或消费上限，需要账户侧处理。 |

第一轮 LSP 会话测试还暴露了两项夹具假设：Node 普通子进程由 libuv 自己的
kill-on-close Job 管理，父退出场景需要保留一个仍属于 Narrant 准确外层 Job 的叶进程；
32个短命子进程的累计数也不能直接猜成34。夹具已修正并加入准确归属断言，
普通短命子进程仍按原方式启动，最终严格清理结果没有放宽。
来源：[libuv Windows 进程实现](https://github.com/libuv/libuv/blob/v1.51.0/src/win/process.c)。

`5d66c8c` 的 raw Win32 探针在显式持有根/叶两个对象时，准确 Job 列表却包含三个真实 PID，
并收到额外 PID 的独立 NEW_PROCESS 通知。因此它证明存在额外对象，尚未证明嵌套关联重复计数，
也未识别额外对象的映像。`ebabed5` 仅将诊断夹具改为不创建控制台的 detached 启动，
并增加额外 PID 的映像/归属/Wait0 诊断；产品 flags 与回收算法不变。
该修正通过 Windows-target 编译检查，真实 Windows 运行因上述计费错误尚未执行。

本次本地验证：库检查、LSP聚焦测试17通过、验收/探针脚本17通过，
全量 Rust 753通过/0失败/81个既有固定工具测试 ignored，另8项真实helper集成测试默认ignored。
固定工具的实际执行以各CI运行记录为准；本地结果不能替代 Windows 生命周期验证。
剩余工作是恢复CI后验证诊断修正、22项LSP会话与批次、错误兜底，再决定并验证共享owner接入。
草稿PR和原失败断言保留，尚未完成共享修复。

## 2026-10-03 最终新增原生链路验收：4c65bb8

实际代码提交 `4c65bb871e9bfa2f7a8901bd60f8c37ed8dc3413`，
[运行 37111018854](https://github.com/XLCYun/narrant/actions/runs/37111018854)。
Windows新native owner的产品探针与raw1816门禁通过，四包构建和ready、四语言套件及以下所有新链路组均执行通过。

| 验收组 | Windows x64 | macOS ARM64 / SDK15.5 |
| --- | --- | --- |
| Go原生套件 | 通过，2.292秒 | 通过，0.561秒 |
| Java | 25通过，42.024秒 | 25通过，17.000秒 |
| C# | 29通过，16.600秒 | 29通过，11.588秒 |
| Clang | 25通过、1个macOS专用跳过，7.823秒 | 25通过、1个Windows专用跳过，5.718秒 |
| 新native Host | 19通过、1个子进程fixture ignored，3.53秒 | 17通过、0ignored，3.42秒 |
| factory | 2通过 | 2通过 |
| 真实四包生产安装 | 1通过、0ignored，207.19秒 | 1通过、0ignored，91.22秒 |
| 公共CodeGraph/diff | 8通过、0ignored，22.48秒 | 8通过、0ignored，15.08秒 |

Windows图测试通过显式 `--ignored` 真正执行全部8项，未因缺工具或链接失败跳过。
`narrant`二进制在测试模式下复用Tauri已有manifest，库/集成测试保留CommonControls manifest；
默认应用构建没有新增覆盖参数。此轮已验证真正MSVC链接成功，不只依赖本机参数路由实验。

[原生Windows job 111168782156](https://github.com/XLCYun/narrant/actions/runs/37111018854/job/111168782156)
的最终结论仍为failure：旧共享Job的两项为1通过、1失败/0.10秒，
同一后代句柄立即等待仍为258而非0。最终 `AggregateError: 1 native validation group(s) failed`
只包含旧共享组，没有将它隐藏、移除或用新native成功抵消。
[原生macOS job 111168782287](https://github.com/XLCYun/narrant/actions/runs/37111018854/job/111168782287)为success。

[Python Windows 111168782367](https://github.com/XLCYun/narrant/actions/runs/37111018854/job/111168782367)
三组20/14.96秒、3/2.94秒、4/3.82秒通过；
[Python macOS 111168782294](https://github.com/XLCYun/narrant/actions/runs/37111018854/job/111168782294)
三组35/8.56秒、3/2.64秒、4/5.86秒通过，均零失败、零ignored。
完整官方日志位于 `/private/tmp/narrant-ci-native-windows-4c65bb8.log`，同轮其余日志为
`narrant-ci-macos-4c65bb8.log`、`narrant-ci-python-windows-4c65bb8.log`、`narrant-ci-python-macos-4c65bb8.log`。

### 仍存在的旧共享缺陷

起点 `6761aca6907a3fd2f1ca730fa6a180656f132e7f` 已包含相同回收算法和
`lsp/tests/windows_process.rs` 的严格立即退出断言（来源提交 `fb76bd0`）。
截至 `4c65bb8`，本分支对旧Windows路径只改过模块/方法可见性，未改变其算法；此前真实探针已分别复现
活跃数归零及PID列表为空时原HANDLE仍为258的短暂窗口。未独立运行起点SHA的Windows任务，
所以不声称完成该基线实测、不声称永久进程泄漏，也不声称本次总CI已绿。
该轮新增helper使用独立owner，旧LSP/Python行为保持原样；旧缺陷须在后续共享路径工作及合并门禁中处理。
此前拒绝的共享接入补丁未应用。下文保留所有历史失败与范围决策。

## 2026-10-03 运行记录：7419870

[运行 37109657139](https://github.com/XLCYun/narrant/actions/runs/37109657139) 已全部结束。
本提交只让独立验收组串行跑完并汇总失败，产品源码未变。

[原生 Windows 111164937810](https://github.com/XLCYun/narrant/actions/runs/37109657139/job/111164937810)
的实际 native owner 产品探针通过：父退出保留管道4毫秒、关闭管道3毫秒、父仍存活16毫秒，
返回后父子原句柄均立即为0，raw1816门禁通过。Node35项中34通过、1个POSIX专用跳过。
四包构建和ready通过；Go2.276秒、Java25/56.256秒、C#29/21.676秒、Clang25通过加1个平台跳过/9.216秒。

| Windows 独立验收组 | 实际结果 |
| --- | --- |
| 旧共享 Job | 1通过、1失败、0ignored，0.13秒；第85行同一后代句柄为258而非0 |
| 新 native Host | 19通过、0失败、1ignored，4.24秒；ignored仅为由其他测试启动的子进程夹具 |
| native factory | 2通过、0失败、0ignored |
| 真实四包生产安装 | 1通过、0失败、0ignored，165.90秒 |
| 公共 CodeGraph/diff 八图 | 构建失败，八项均未执行 |

图验收命令同时链接 `narrant` 应用二进制，链接器报告
`CVTRES fatal error CVT1100: duplicate resource. type:MANIFEST, name:1, language:0x0409`
及 `LINK LNK1123`。显式测试环境的 `/MANIFEST:EMBED` 与 Tauri 的应用资源同时参与链接；
当前修复这一测试构建边界，不删除图验收或改变应用默认manifest。
最终 `AggregateError: 2 native validation group(s) failed` 准确保留共享回归与图构建两个失败。
完整官方日志：`/private/tmp/narrant-ci-native-windows-7419870.log`。

同轮[原生 macOS 111164937930](https://github.com/XLCYun/narrant/actions/runs/37109657139/job/111164937930)
完整成功：Go0.590秒、Java25/19.302秒、C#29/12.324秒、Clang25通过加1个平台跳过/5.744秒；
Host17/3.54秒、factory2、安装1/88.53秒、公共图8/14.95秒通过。
[Python Windows 111164937942](https://github.com/XLCYun/narrant/actions/runs/37109657139/job/111164937942)
三组20/21.67秒、3/3.87秒、4/4.71秒通过；
[Python macOS 111164937956](https://github.com/XLCYun/narrant/actions/runs/37109657139/job/111164937956)
三组35/8.68秒、3/10.84秒、4/4.01秒通过。以上成功Rust组均零失败、零ignored。

## 2026-10-03 运行记录：6bc50d3

[运行 37108089607](https://github.com/XLCYun/narrant/actions/runs/37108089607) 已全部结束。
`6bc50d3` 仅修正验收脚本测试对 CRLF 的接受；专用 native owner 产品源码仍为 `d2b0245`。

- [原生 Windows 111160449171](https://github.com/XLCYun/narrant/actions/runs/37108089607/job/111160449171)
  失败。实际产品 owner 的三个生命周期探针均立即观察父子原句柄为0，耗时3/2/4毫秒，
  raw1816门禁通过；Node32项中31通过、1个平台跳过。四包构建和ready通过，Go1.788秒、
  Java25/42.867秒、C#29/16.649秒、Clang25通过加1个平台跳过/7.803秒。
  随后旧共享 Job 两项回归为1通过、1失败/0.12秒；
  `windows_job_reclaims_child_after_parent_exit` 在第85行观察到258而非0。
  新 native Host、factory、生产安装与八项公共图均未执行，不能拼接旧结果宣称本轮通过。
- [原生 macOS 111160449150](https://github.com/XLCYun/narrant/actions/runs/37108089607/job/111160449150)
  成功。四包与ready通过，Go0.556秒、Java25/16.928秒、C#29/11.181秒、Clang25通过加1个平台跳过/5.039秒；
  Host17/3.33秒、factory2、生产安装1/82.39秒、公共图8/13.23秒通过。
- [Python Windows 111160449124](https://github.com/XLCYun/narrant/actions/runs/37108089607/job/111160449124)
  成功，Rust三组20/21.52秒、3/4.29秒、4/6.26秒通过；
  [Python macOS 111160448968](https://github.com/XLCYun/narrant/actions/runs/37108089607/job/111160448968)
  成功，三组35/8.51秒、3/2.93秒、4/5.70秒通过。上述成功Rust组均零失败、零ignored。

完整官方日志位于 `/private/tmp/narrant-ci-native-windows-6bc50d3.log`，同轮其余日志分别为
`narrant-ci-macos-6bc50d3.log`、`narrant-ci-python-windows-6bc50d3.log`、
`narrant-ci-python-macos-6bc50d3.log`。下一步只改验收组的失败汇总，让独立组继续串行执行，
任何失败仍使整体非零退出；不删除、放松或忽略旧共享回归，也不将其已知缺陷归为通过。

## 已取得的 CI 结果

2026-10-03（北京时间），提交 `9ea5acb` 的
[macOS 原生分析任务](https://github.com/XLCYun/narrant/actions/runs/37045220272/job/110964856515)
完整成功，耗时 9 分 6 秒。使用 Xcode16.4 / SDK15.5，完成四包构建、解包和启动，
Go 原生套件及 Java22 / C#24 项测试通过。Clang 为 22 通过、1 项 Windows SDK 专用测试预期跳过。
Rust Host16、factory2 项通过，
真实四包生产安装用例通过（91.05 秒），公共 CodeGraph/diff 七项通过（12.91 秒），没有跳过。
同轮既有 Python/macOS 任务也通过。

该轮 workflow 整体因 Windows 脚本失败而失败：嵌套运行时的执行标记依赖 POSIX mode，
测试也使用了 Windows 无法直接执行的 shell fixture。`974054b` 按目标平台识别 Windows
`.exe`，保留 macOS 执行位检查，并让 Windows 测试真正执行打包后解出的程序。
[修复后的自动运行](https://github.com/XLCYun/narrant/actions/runs/37046327780) 已通过两个
Windows job 原来失败的 Node 步骤；后续状态见下文，不将步骤通过等同平台验收通过。

`974054b` 的[macOS 原生分析任务](https://github.com/XLCYun/narrant/actions/runs/37046327780/job/110968552316)
随后也完整通过：四包构建、Go 套件、Java22 / C#24、Host16 / factory2 均成功；
Clang 同样为 22 通过、1 项 Windows SDK 专用测试预期跳过。
生产四包安装 1 项（81.42 秒）及公共图/diff 7 项（12.22 秒）通过，这两组零跳过。
这轮结果包含 Windows 打包元数据修复；既有 Python/macOS 任务同样成功。
既有 [Python/Windows 任务](https://github.com/XLCYun/narrant/actions/runs/37046327780/job/110968552230)
也已通过，Rust 分组分别为 20、3、4 项，全部零失败、零跳过。

最新提交 `802fadf` 的[macOS 原生任务](https://github.com/XLCYun/narrant/actions/runs/37053187055/job/110991346136)
于 2026-10-03 03:25（北京时间）完整成功。六份固定开发构件均下载、校验、解包成功，
包含新 net8 引用包；四个支持包构建成功。真实 Java25 / C#29 项通过，Go 套件成功，
Clang22 项通过、1 项 Windows SDK 专用测试预期跳过；Host16 / factory2 通过。
生产四包安装 1 项（84.24 秒）及公共图/diff 8 项（14.79 秒）通过，两组零跳过。
本轮直接包含 C# 两目标隔离、完整失败及执行哨兵增量，日志保留在
`/private/tmp/narrant-ci-macos-37053187055.log`。既有 Python/macOS 任务也通过。
同轮 [Python/Windows 任务](https://github.com/XLCYun/narrant/actions/runs/37053187055/job/110991346412)
也完整通过，Rust 三组 20/3/4 项均零失败、零跳过；日志为
`/private/tmp/narrant-ci-python-windows-37053187055.log`。这不代替新增四语言的 Windows 验收。

`802fadf` 的任务已覆盖旧基线的全部检查，因此协调者在 03:35（北京时间）取消仅剩旧 Windows
任务的 `37046327780`，停止过时的重复运行；最新 `37053187055` 保持执行。旧任务结果是
**cancelled**，不是测试失败或成功。官方终结日志表明前四种工具迅速进入下一步，LLVM 准备从
02:18 到取消时未结束；旧脚本没有子阶段计时，不能据此判断下载、校验还是解包慢。
最新脚本已有三个阶段的计时，随后取得如下定位结果。

`802fadf` 的 [Windows 原生任务](https://github.com/XLCYun/narrant/actions/runs/37053187055/job/110991346331)
于 05:18（北京时间）到达任务的 120 分钟期限，官方结论为 **cancelled**；本轮没有人为取消。
LLVM 在 03:19:46.939 完成下载、03:19:48.063 完成 SHA-256 校验，之后一直没有解包完成标记，
直到 05:17:58 runner 取消。其余五份构件均已完成下载、校验和解包。
因此问题定位在 Windows `System32/tar.exe` 解包固定 LLVM `.tar.xz` 的阶段；日志尚不能证明
底层是 XZ、libarchive、磁盘或扫描的哪一种原因。该轮未执行 SDK 选择或四语言原生测试。
完整官方日志保留为 `/private/tmp/narrant-ci-native-windows-37053187055.log`。

修复只替换 Windows `.tar.xz` 的提取路径：使用 runner 已有的 7-Zip，以两个直接启动的进程
连接 XZ 解码与 TAR 提取，二进制流传输，不生成巨型中间 TAR。两个进程都成功关闭才算解包
成功；30 分钟独立期限触发后回收进程树，回收未确认则保留临时目录并失败。LLVM 下载源、
摘要、版本和预期目录不变，其他归档格式及 macOS 继续使用原路径。
本机进程生命周期测试不能证明 Windows 解包成功；CI 强制运行真实 7-Zip 小归档测试，
随后仍须完成同一官方 LLVM 归档及完整四语言验收。

修复 `9e164b7` 随 `6d1081a` 推送后，
[Windows 任务 111039380577](https://github.com/XLCYun/narrant/actions/runs/37067660266/job/111039380577)
确认真实 7-Zip 小归档测试通过（306.6883 毫秒），runner 使用 7-Zip 26.03。
完整 LLVM 归档解包成功，耗时 87.0 秒，六份工具链全部准备成功。
任务随后在 SDK 目录检查失败：固定 MSVC Tools 14.44.35207 的 include 和 Windows SDK
10.0.26100.0 的 ucrt/um/shared 均存在，但 `VC/Redist/MSVC/14.44.35211/x64/Microsoft.VC143.CRT`
不存在。该值此前来自 runtime 版本清单，尚需核实其与分发目录版本的映射，不能猜测另一目录。
本轮仍未执行 Windows helper 构建；完整日志为 `/private/tmp/narrant-ci-native-windows-37067660266.log`。
同轮 macOS 原生任务完整成功：Java25、C#29、Clang22 通过及1项 Windows 专用跳过，
Host16、factory2、生产安装1（96.63秒）、公共图8（16.91秒）通过。
既有 Python 双平台任务也成功；这轮 workflow 总体仍因上述 Windows 原生任务而失败。

## 固定构件与隔离边界

`resources/native-toolchains.json` 为 macOS ARM64 与 Windows x64 分别固定官方构件 URL、
版本和摘要。Go 1.23.8、Temurin JDK 25.0.4.1+1（执行工具）、JDK 17.0.20.1+1
（分析目标声明）、LLVM 21.1.8 使用 SHA-256；.NET SDK 10.0.100 使用官方 SHA-512。
另外固定跨架构的 `Microsoft.NETCore.App.Ref` 8.0.0 官方 NuGet 标准引用包，SHA-512 与
不可变 catalog 记录对照；只将 `ref/net8.0` 及许可证放入 C# 支持包，不带入该包的 targets
或 analyzers。其目录与 SDK 自带的 net10.0 引用目录独立，执行 helper 的 runtime 仍为 10.0.0。
JDT 3.47.0 的构建 JAR 闭包另由 `native-helpers/java/dependencies.lock.json` 固定。

`scripts/provision-native-toolchains.mjs` 顺序下载到 runner 临时目录，流式验证摘要后解包，
不运行安装器、不修改全局工具链、不解析业务依赖。安装目录已存在时失败，不盲信旧内容。
归档解包完成后删除下载文件，避免 LLVM 压缩包持续占用额外空间。输出私有工具路径到
`environment.json` 与 Actions 的 `GITHUB_ENV`，没有把被分析仓库加入任何依赖来源。

Windows 使用 LLVM 的 `clang+llvm-21.1.8-x86_64-pc-windows-msvc.tar.xz` 开发归档。
官方说明该归档包含 LLVM 开发所需的额外库，普通安装器面向使用编译器的用户，不能据此
推断安装器拥有 LibTooling 开发库。helper 仍链接 LibTooling，不使用 AST JSON 替代。
来源：[LLVM 21.1.8 官方发布说明](https://github.com/llvm/llvm-project/releases/tag/llvmorg-21.1.8)。

## 目标 SDK 和构建前提

| CI 目标 | 固定声明与构建环境 | 验证边界 |
| --- | --- | --- |
| macOS ARM64 / `macos-15` | Xcode 16.4，macOS SDK 15.5 | 固定 `Xcode_16.4.app` 下 SDK，缺失直接失败；与本机 SDK 27.0 的验收分开记录 |
| Windows x64 / `windows-2022` | VS 2022 Enterprise，MSVC Tools 14.44.35207，runtime 版本配置 14.44.35211，Windows SDK 10.0.26100.0 | 编译器、SDK、runtime 元数据及十个私有 CRT DLL 的版本已通过真实检查 |

macOS runner 的官方目录列出 Xcode 16.4 和 SDK 15.5。Windows Server 2022 runner 目录（镜像 20260927.320.1）列出
VS 2022 17.14 系列、Windows SDK 10.0.26100.0 和 VC runtime 14.44.35211；其 README
未提供默认 MSVC Tools 目录的完整版本，14.44.35207 已由本次 runner 的真实目录检查确认。
runtime 清单中的产品版本不能直接当成 Redist 目录名，也不能代替私有 CRT DLL 的版本检查。
微软自身项目通过 `VC/Auxiliary/Build/Microsoft.VCRedistVersion.default.txt` 读取目录标识再定位
CRT；该标识与 Tools 版本独立。[Microsoft Xbox GDK 示例](https://github.com/microsoft/Xbox-GDK-Samples/blob/cdbec9e9b95fdf931b0802fc429fc3719c2efa47/Samples/Tools/CMakeXboxConsoleApp/CMakeGDKXbox.cmake#L65-L70)、
[Microsoft DbgShell 示例](https://github.com/microsoft/DbgShell/blob/7f98155b863c00295cd24513cc2a4c1c0fdf346c/DbgEngWrapper/DbgEngWrapper.vcxproj#L79-L84)。
修复 `8d896f4` 按该元数据选择唯一 CRT 根，并校验目录内全部 DLL 的数值版本前三段是否等于
配置 `14.44.35211`，同时记录完整四段版本。元数据缺失、越界、必需 DLL 缺失或版本不符均失败，
诊断中的目录列表仅用于定位，不用于选择最新版本。本机专项 11 项通过；Windows 版本资源
读取测试为 CI 必跑项，不能用 macOS 的该项跳过代替实际私有 CRT 验证。
后续 `4fa0f0d` 的真实 Windows 任务已确认元数据为 `14.44.35112`，对应 CRT 目录中的十个
DLL 均为 `14.44.35211.0`，目录与版本校验全部通过。LLVM 解包同轮耗时 34.4 秒。
Go、Java、C# 包构建和 ready 烟测已通过；Clang 编译后链接失败，LLVM 导出依赖指向其构建
机器上的 `VS2019/Professional/DIA SDK/lib/amd64/diaguids.lib`，本 runner 使用 VS2022。
目前定点修正该开发库依赖映射；尚未执行后续 Windows 语言套件、Host、安装和公共图测试。
见[真实 Windows 任务](https://github.com/XLCYun/narrant/actions/runs/37069139995/job/111044162856)，
完整日志为 `/private/tmp/narrant-ci-native-windows-37069139995.log`。
`bf8f9d1` 的修复只调整本次 CMake 配置中 `LLVMDebugInfoPDB` 的固定旧 DIA 依赖项，替换为
已指定的 `CMAKE_GENERATOR_INSTANCE` 下对应库；要求库存在且旧项恰好出现一次，保留其他依赖
及顺序，不改写 SDK 文件。真实 CMake 导入目标回归先复现失败，修复后替换、缺库、意外导出
三个用例通过；既有 unittest discover 会在两平台运行它们。实际 Windows 链接仍需后续 CI。
同轮 macOS 原生任务完整通过，生产安装 1 项（86.88秒）、公共图 8 项（13.79秒）均零跳过；
Java25、C#29、Clang22通过加1项Windows专用跳过、Host16/factory2维持成功。
Python Windows 的 Rust 20/3/4 项也全部通过（19.24/2.90/4.93秒），零失败、零跳过。
`0e6f8e8` 的[下一轮 Windows 任务](https://github.com/XLCYun/narrant/actions/runs/37071028213/job/111050242159)
确认 DIA 映射、Clang 链接成功，四包均完成构建及 ready 烟测。第一组 Go 测试随后在
`TestWorkspaceUnion` 失败：fixture 总是追加宿主条目，在 Windows 上得到 host、linux、windows
三个配置，而用例预期两个。`f749fbc` 仅把该 fixture 限定为要验证的 linux/windows 两项，
生产配置发现和并集逻辑未变；三个相关测试在本机通过。后续 Windows Java/C#/Clang 套件、
Host、安装及公共图测试尚未执行。完整日志为 `/private/tmp/narrant-ci-native-windows-37071028213.log`。
同轮 macOS 全流程通过，Clang 加上 CMake 回归后共 25 项通过、1 项 Windows SDK 专用跳过；
Java25、C#29、Host16/factory2、安装1（86.82秒）、图8（14.02秒）通过。Python Windows 20/3/4
也通过（21.26/4.03/6.40秒），零失败、零跳过。
`2e56a6e` 的[Windows 任务 111053944871](https://github.com/XLCYun/narrant/actions/runs/37072185184/job/111053944871)
确认 Go 套件（2.498秒）、Java25（56.172秒）、C#29（20.774秒）全部通过。
Clang26 项中 24 通过、1 项跳过、1 项错误：Unicode 路径测试的 Python stdout 管道默认使用
Windows cp1252，无法解码 helper 的 UTF-8 输出。修复只将该测试驱动的三条管道显式设为严格
UTF-8，不改变 helper 生产协议。Rust Host、factory、安装和八项图尚未执行；完整日志为
`/private/tmp/narrant-ci-native-windows-37072185184.log`。
同轮 macOS 原生任务完整通过（Clang25通过+1跳过、安装1/89.70秒、图8/16.19秒）；既有
Python Windows 的20/3/4项也通过（22.53/4.29/7.33秒），零失败、零跳过。
`360cd26` 的[Windows 任务 111058341797](https://github.com/XLCYun/narrant/actions/runs/37073573624/job/111058341797)
确认严格 UTF-8 测试驱动修复有效：Go（1.817秒）、Java25（44.341秒）、C#29（18.745秒）
成功，Clang26项中25通过、1平台跳过（7.508秒），Unicode 用例通过。
随后首次执行的 Rust Host 分组为6通过、9失败、0跳过（5.61秒）：Node24夹具在协议启动前
发生 `ncrypto::CSPRNG(nullptr, 0)` 断言并以134退出，多个用例因此缺少协议消息或PID文件。
生产 factory 原本显式传入 Windows `SystemRoot`，Node 夹具和公共图直连测试入口遗漏了该项；
修复让这两个测试调用方采用相同的最小环境，保留 Host 的 `env_clear` 及其他注入变量隔离。
新增真实 Session 回归检查 crypto、显式环境、scratch 与进程退出，本机 Host17项通过。
实际 Windows 仍须复验该差异能否消除 CSPRNG 失败；factory、生产安装、公共图未执行。
完整日志为 `/private/tmp/narrant-ci-native-windows-37073573624.log`。
同轮 macOS 原生全流程成功，Host16、factory2、生产安装1（90.57秒）、公共图8（15.55秒）
均零跳过；既有 Python 双平台也成功，Windows 的20/3/4项零失败、零跳过。
`d498548` 的[Windows 任务 111066344312](https://github.com/XLCYun/narrant/actions/runs/37076131635/job/111066344312)
再次通过全部四语言原生套件：Go（2.442秒）、Java25（56.780秒）、C#29（21.568秒）、
Clang25通过及1项macOS平台跳过（10.053秒）。Node CSPRNG 启动故障已消除，新增最小环境
回归通过；Host16项中15通过、1失败（3.60秒）：`subprocess_reaps_descendants_after_parent_exit`
预期错误却得到成功。Node/libuv 在 Windows 上默认将普通子进程纳入随父退出清理的内部 Job，
使夹具不能可靠制造遗留后代。修复仅在 Windows 设置 detached 来绕过 Node 的内部 Job，
不请求从 Rust 的外层 Job breakaway；后代自行写入就绪 PID 后父进程才退出，保留两种管道模式
的退出结果和立即回收断言。本机 Host17项通过，Windows 仍需实际复验。
机制依据：[Node24/libuv 子进程实现](https://github.com/nodejs/node/blob/v24.18.0/deps/uv/src/win/process.c#L1052-L1102)。
该轮未运行 factory、安装和公共图。
完整日志为 `/private/tmp/narrant-ci-native-windows-37076131635.log`。
同轮 macOS 全流程通过：Host17（3.82秒）、factory2、安装1（88.61秒）、公共图8（13.86秒）
均零跳过；Python Windows 的20/3/4项也通过（21.67/5.28/9.76秒）。
`d628bfd` 的[Windows 任务 111075220175](https://github.com/XLCYun/narrant/actions/runs/37079022600/job/111075220175)
通过四语言原生套件：Go（1.882秒）、Java25（35.123秒）、C#29（13.003秒）、Clang25通过
及1项macOS平台跳过（6.559秒）。Host16项中14通过、2失败（3.28秒），分别为正常返回及
Drop/panic 后的进程存活断言：`WaitForSingleObject(..., 0)` 返回258而非0。
现有断言只按PID重新打开进程，且错误没有区分父/后代，因此该日志尚不能单独证明后代逃逸
或生产回收失败。后续将保留真实进程句柄、核实准确Job归属并观察退出信号，缩短诊断流程。
该轮 factory、生产安装、公共图仍未执行；完整日志为 `/private/tmp/narrant-ci-native-windows-37079022600.log`。
同轮 macOS 全流程通过（Host17、factory2、安装1/90.83秒、图8/13.61秒，Rust零跳过）；
Python Windows 的20/3/4项通过（21.25/3.39/4.69秒），零失败、零跳过。
诊断改动 `b0ed324` 新增独立 Windows 进程探针，直接引用生产 `WindowsJob`，在 SDK 下载和
Tauri 编译前运行。三个场景记录后代的准确 Job 归属、回收前固定的进程句柄、立即退出信号和
Job 活跃计数；原 Host 测试同步保留父子句柄，原有立即回收断言仍然有效。生产回收算法未改。
本机 Windows 目标编译检查、两项 macOS 生命周期测试和九项 CI 接线测试通过；真实 Windows
行为仍待运行证据，短探针不替代完整安装和公共图验收。
`76f0204` 的[Windows 早期诊断任务 111087221282](https://github.com/XLCYun/narrant/actions/runs/37082946157/job/111087221282)
在探针编译成功后，于 Node 可执行路径检查退出：`NARRANT_TEST_NODE must be an existing absolute executable`。
三个 Job 场景均未执行，不能据此判断进程回收根因；也未进入 SDK、helper、Host、安装及图验收。
本轮日志未打印实际变量值，因此尚不能区分 PowerShell 命令解析与 MSYS 路径转换。
后续诊断入口将沿用已有完整验收的 Node `process.execPath` 与直接子进程环境传递，并输出路径检查证据。
`e1f8985` 的[Windows 早期诊断任务 111089160997](https://github.com/XLCYun/narrant/actions/runs/37083593430/job/111089160997)
已通过 Node 路径入口，并在首个父退出场景复现生产同步问题：父 PID4632、后代 PID7116
均属于准确 Job，清理前活跃计数3。`terminate_and_wait` 返回成功且计数为0，
但清理前固定的后代句柄立即 `WaitForSingleObject(..., 0)` 仍为258，父句柄为0。
因此在此实际场景中，PID复用及Job逃逸均已排除，活跃计数归零不足以证明全部进程对象退出。
完整日志 `/private/tmp/narrant-ci-native-windows-37083593430.log`；仅首场景执行，后续完整验收尚未运行。
上一轮 `76f0204` 的 macOS 已完整通过：Host17（3.61秒）、factory2、安装1（84.14秒）、
图8（14.47秒），Rust零跳过；在替代运行启动后，旧轮剩余 Python Windows 任务被手动取消，
其取消结果不计为通过。
共享生产 `WindowsJob` 的候选修改尚未应用：自动审批要求先验证隔离机制并明确批准其对现有
Windows LSP/Python 的影响。新增探针先独立测试零活跃进程上限的读回、拒绝新建及保留已有进程，
再执行原有三项生产回收硬断言；机制验证通过不会替代回收验收。候选生产补丁另行审查。
`e244845` 的[Windows 门禁诊断任务 111092472010](https://github.com/XLCYun/narrant/actions/runs/37084709435/job/111092472010)
在首个准入门禁场景等待标记10秒后超时；已有日志只记录父子准确Job归属及清理前状态，
没有取得零上限读回、拒绝新建的完整证据，原有三项清理场景尚未执行。
后续仅在独立探针增加设置/读回阶段、同步异常、父子状态与有界错误输出，生产仍未修改。
惰性候选补丁审查另有一项未闭合问题：清理时枚举可能遗漏已经进入退出流程的成员，
因此候选尚不能作为满足完整回收合同的最终修复。
`0671755` 的[Windows 诊断任务 111094268752](https://github.com/XLCYun/narrant/actions/runs/37085313601/job/111094268752)
读回门禁 flags=0x2008、active_limit=0，已有父子句柄仍为258；Node 创建请求同步抛出 UNKNOWN。
该错误尚未包含原始 Win32 错误码，保留为诊断失败，不能据此声称门禁或完整回收已经通过。
同轮 macOS 全流程成功（Host17、factory2、安装1/83.03秒、公共图8/13.99秒，Rust零跳过），
Python Windows 的20/3/4项也通过（22.78/3.41/4.76秒）。
2026-10-03 用户已明确授权必要的共享 Windows LSP/Python 回收修复，保持原 helper 后代能力；
此前审批阻断解除，没有采用单进程限制。新探针先固定原立即 Wait(0) 结果，再观察准确 Job PID列表
与同一句柄的 wait/list/wait，诊断延迟不能掩盖原红灯。现有证据只证明活跃计数不足，尚未实测
证明 PID列表会遗漏未退出对象。新的完整性方案在设计上使用全生命周期保留句柄和累计
TotalProcesses 对账，通知丢失或无法捕获对象必须明确失败；实现与共享回归仍须验证。
`817b3bb` 的[Windows 诊断任务 111152999508](https://github.com/XLCYun/narrant/actions/runs/37105473421/job/111152999508)
取得更直接的退出次序证据：清理前成员列表为[6736]；清理返回后立即保存的后代句柄状态为258，
随后同一次采样是 wait_before=258、members=[]、wait_after=258、exactJob=true、active=0。
下一次采样才变为0。原硬断言仍根据最初258失败，没有用诊断等待把结果改为通过。
因此成员列表为空也不足以证明已经持有的对象完成退出，生命周期跟踪的完整性问题已有实测依据。
这只证明短暂的退出同步窗口，不证明进程持续执行或永久泄漏。日志为
`/private/tmp/narrant-ci-native-windows-37105473421.log`；本轮后续完整原生验收未运行。
共享实现接入再次被自动审批拒绝，理由是影响现有 LSP/Python 且机制尚缺真实验证；用户授权仍有效。
当前先在独立探针验证生命周期跟踪与累计对账，生产清理尚未接入该候选机制。
候选探针已接入三种父子生命周期及真实漏捕获反例：保存准确进程对象，按统一两秒期限
封闭创建、终止、停止监听、等待并核对累计数量；故意漏掉的短命成员必须导致错误且重试仍报错。
独立 Rust 门禁夹具直接检查 raw OS error=1816，保留已有父子存活与准确 Job 归属断言。
本机 Windows 目标编译、Rust 格式和 JS 语法检查通过，实际候选执行尚待 CI。
默认先运行候选，再运行原生产三个场景；生产红灯仍会使整个任务失败，不会被候选通过覆盖。
探针通过握手阻止根进程提前创建后代，不能替代未来生产在 Assign/Resume 前建立跟踪的验证。
候选停止后不再排空通知，漏消息由补捕和累计计数检测；Drop 最终 join 仅为探针避免遗留线程的兜底，
对任意调度/API异常不承诺硬两秒墙钟上限，生产接入仍需独立审查与回归。
同轮其余三项任务全部成功：macOS 原生 Go0.549秒、Java25/17.380秒、C#29/11.330秒、
Clang25通过+1平台跳过、Host17/3.70秒、factory2、安装1/80.42秒、公共图8/15.85秒。
Python Windows20/34.98秒、3/3.78秒、4/4.47秒通过；Python macOS35/3/4项通过，
各 Rust 分组零失败、零跳过。该运行已全部结束，只有原生 Windows 早期探针失败。
`6b18b68` 的[候选 Windows 任务 111156082562](https://github.com/XLCYun/narrant/actions/runs/37106542040/job/111156082562)
已实际通过三种候选生命周期：每次保留3个对象、累计3、活跃0，父子句柄立即为0，耗时3/3/9毫秒。
故意漏掉真实短命成员时保留3、累计4，返回覆盖错误且重试仍报错。Rust 门禁得到准确1816，
已有父子仍为258，验证成功。随后原共享生产场景仍在立即258断言处失败，整个任务保持失败；
候选通过没有覆盖生产红灯。完整日志 `/private/tmp/narrant-ci-native-windows-6b18b68.log`。
在该实测之后，共享接入仍被自动审批拒绝，理由是对所有既有 LSP/Python 会话的影响未充分验证；
被拒绝补丁未应用。当前采用实质更小的范围：只在 `native_analyzer` 内新增专用 Windows 所有者，
迁入单份已验证跟踪器，保持首次 Assign 前附加端口、Resume 前固定根对象；旧 LSP/Python 行为不变。
保留 helper 后代能力，清理时才封闭新成员。代价是有限重复悬挂启动原语；旧共享完成次序缺陷
没有因此修复，既有回归不删除、不放松。专用实现已完成，本机库检查及17项 Host 聚焦测试通过，
新 owner 的 Windows 目标编译通过，实际平台运行与最终差异审查仍待完成。
原生探针已直引新的专用所有者并移除候选副本；生产只维护一份跟踪器。
新增三个 Windows 测试覆盖执行前入组、真实短命成员缺口和终止前失败时的 direct Child 回收，
另一个 ignored 函数仅用作子进程夹具，不能被计作跳过了实际验收用例。
同轮 `6b18b68` macOS 完整通过（Host17、factory2、安装1/83.04秒、公共图8/14.62秒），
Python Windows20/27.80秒、3/3.53秒、4/9.85秒及 macOS35/3/4项通过；所有任务已结束。
`d2b0245` 的[实际专用 owner 任务 111159181765](https://github.com/XLCYun/narrant/actions/runs/37107634743/job/111159181765)
已通过真正产品代码的早期探针：父退出保留管道、父退出关闭管道、父仍存活三场景均在返回后立即
观察父子原 HANDLE 为0，cleanup耗时3/3/7毫秒；raw1816门禁也通过。日志为
`/private/tmp/narrant-ci-native-windows-d2b0245.log`，不再是候选副本结果。
后续 Node 脚本测试为28通过、1失败、1平台跳过；唯一失败是 CI 接线测试使用只匹配LF的正则，
不接受 Windows 检出的 workflow CRLF。生产回收没有再次失败；SDK、共享LSP回归、Host、安装和图
仍未执行，不能提前标记平台完成。当前只修正测试的换行兼容性，所有路径与验收断言保留。
当前 `windows-2025` 标签已指向 VS 2026 镜像，所以本工作流明确选择官方仍配备 VS 2022 的
`windows-2022`，没有改写目录猜测或静默回退。映射见 [官方 runner 标签表](https://github.com/actions/runner-images#available-images)。
来源：[macOS runner 镜像](https://github.com/actions/runner-images/blob/main/images/macos/macos-15-arm64-Readme.md)、
[Windows runner 镜像](https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md)。

Windows 分别授权 `Windows Kits/10/Include/10.0.26100.0` 和
`VC/Tools/MSVC/14.44.35207/include`。前者是 `clang-target`，后者以
`clang-system-headers` 和 `additional_system_platform_ids` 显式关联。
不把 Program Files、SystemRoot 或磁盘根作为受信 SDK 根；也不运行 compilation database
中的编译器。构建 helper 的 CMake 使用固定 Visual Studio generator、SDK 和 toolset，
这些是本工具的构建输入。Microsoft 说明命令行构建依赖开发工具环境，不能把分析程序的
环境探测当成配置发现依据。来源：[Microsoft C++ 命令行构建文档](https://learn.microsoft.com/en-us/cpp/build/building-on-the-command-line?view=msvc-170)。

## 必须执行的验收

PR 自动触发 `Language support packs / native-analyzers` 两个 matrix job。每个平台：

1. 运行构建/验收脚本的 Node 测试，安装固定私有开发工具。
2. 为 Go、Java、C#、C/C++ 逐一构建真实支持包，并从归档解包后运行 helper ready 烟测。
3. 运行 Go 原生单元测试及 Java、C#、Clang 的真实 helper Python 测试。
4. 经 `scripts/cargo-agent.sh` 顺序运行 Host 协议/进程生命周期测试、语言 factory 测试，
   以及忽略态的真实四包安装/租约/factory 用例。
5. 执行 `--test native_helper_graphs -- --ignored --nocapture --test-threads=1`，要求
   四个 helper 配置齐全，实际公共 `NativeLanguageAnalyzer → CodeGraph → diff` 覆盖
   Go、Java、C#、C17、C++20、同仓库合并以及 C# net8/net10 隔离与并集，共八项。
   缺少任意 helper 或必要 SDK 都失败，不选择性跳过。

`node scripts/verify-native-analyzers.mjs <产物目录> --build` 是完整入口；省略 `--build`
可复用已完成的本地产物，但仍执行全部 helper 和 Rust 验收。现有 Python job 保留。
新支持包的 artifact 名称独立，未改为远端发布，也没有触发 quality-gate。

所有 Cargo 命令通过现有 wrapper 且串行等待。Windows Git Bash 若缺少 lockf/flock，
现有 wrapper 会警告并直接执行；本 CI job 拥有独立 checkout，且不会并发 Cargo。
这不代表已经改造或验证 Windows 开发机跨 agent 的全局锁。

## 摘要来源

2026-10-03 读取以下官方机器可读元数据，将构件摘要固化到清单；没有为了获取摘要
在本机下载两套大型 LLVM SDK。CI 首次运行时才执行完整下载、摘要验证和解包。

- [Go 全版本下载清单](https://go.dev/dl/?mode=json&include=all)
- [.NET 10 官方发布元数据](https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json)
- [.NET 8.0.0 标准引用包官方不可变目录记录](https://api.nuget.org/v3/catalog0/data/2023.11.14.13.48.02/microsoft.netcore.app.ref.8.0.0.json)
- [LLVM 21.1.8 GitHub release API](https://api.github.com/repos/llvm/llvm-project/releases/tags/llvmorg-21.1.8)
- [Temurin 25.0.4.1+1 release API](https://api.github.com/repos/adoptium/temurin25-binaries/releases/tags/jdk-25.0.4.1%2B1)
- [Temurin 17.0.20.1+1 release API](https://api.github.com/repos/adoptium/temurin17-binaries/releases/tags/jdk-17.0.20.1%2B1)

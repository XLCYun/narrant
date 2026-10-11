# Rust 原生分析 helper

此独立 crate 将 rust-analyzer `9074e9b4c6bc31d7986ccf4e22d18af70e5508da`
的 `project-model`、`load-cargo`、`ide`、`syntax`、`vfs`、`base-db` 和 `hir-def` 直接链接到同一个程序。
提供只读项目核心、完整 Rust 图投影和 `narrant-rust` 单请求 NDJSON 可执行程序。
应用工厂从 1.1.0 未发布候选加载该程序，经公共 Native Host 构图；旧 Rust LSP 与服务器分发已退役。
本地 macOS ARM64 完成实际支持包安装及 Review 验收，最终双平台 CI 待全部开发与审查完成后执行。

## 构建与验证

helper 使用固定 Rust 1.98.0；被分析项目仍使用其实际工具链，二者不得混用。
首次准备 helper 构建工具链可以运行 `rustup toolchain install 1.98.0 --profile minimal`，
不要修改 rustup 默认版本，也不要为被分析项目自动安装版本或 rust-src。

从仓库根目录执行，所有 Cargo 入口均经过仓库锁和 sccache wrapper：

```sh
scripts/cargo-agent.sh +1.98.0 check --manifest-path native-helpers/rust/Cargo.toml --lib --locked
scripts/cargo-agent.sh +1.98.0 build --manifest-path native-helpers/rust/Cargo.toml --locked
scripts/cargo-agent.sh +1.98.0 test --manifest-path native-helpers/rust/Cargo.toml --lib --locked --no-run
```

使用项目支持的 Node 版本运行 wrapper。构建后运行输出的测试可执行文件，清除 Cargo
构建时注入的 `RUSTUP_TOOLCHAIN`，或显式设为测试项目工具链；生产 helper 保留 Host
提供的运行时覆盖。公共 Host 图测试位于 `native_analyzer/tests/rust_graph.rs`，设置
`NARRANT_RUST_HELPER` 为本地二进制的绝对路径，再筛选并运行这些 ignored 集成测试。
正常支持包入口为 `node scripts/build-language-support-packs.mjs --id rust --native`；
`node scripts/verify-native-analyzers.mjs <产物目录> --language rust` 包含真实工厂、安装和 Review。
工厂测试的 `NARRANT_RUST_PACK` 指向完整安装目录，不能仅复制可执行文件。
测试中的标准库/全部依赖案例需要本机项目
默认工具链已有 rust-src 以及标准库 metadata 所需的依赖缓存；测试不自动准备这些组件。
`Cargo.lock` 以固定 revision 的上游锁文件为基线，不要删锁后选择最新传递依赖：
首次新解析的 `unicode-ident 1.0.26` 会与固定 lexer 的 Unicode 断言不兼容。

## 核心接口

`RustProject::load` 接收明确的 Cargo.toml 与 `LoadPolicy`，获得同源的数据库、VFS
和不可变分析快照。`parse` / `source` / `definitions` / `outgoing_calls` /
`parent_modules` 提供直接 API，位置使用 UTF-8 字节偏移，源码 AST 使用本 crate
重导出的 `syntax`。Cargo 根通过 `crates_for` 和 `crate_root` 确认，结果为
`ModuleParent::CrateRoot`，Cargo.toml 不作为源码声明。

- 仅仓库：项目 metadata 无依赖、无 sysroot，始终离线。
- 标准库：项目 metadata `--no-deps`，保留标准库内部依赖，始终离线。
- 全部依赖：按显式网络政策解析项目和标准库依赖。

标准库和全部依赖要求 `ProjectToolchain` 显式提供已有 sysroot 和 rust-src。每档均
禁止 build scripts、过程宏服务和 rustup 自动安装。分析子进程设置 `CARGO_CACHE_RUSTC_INFO=0`，
避免在已有 target 内写入编译器版本缓存；开发编译脚本与缓存配置不变。`test_cfg` 明确控制语义 cfg；
默认启用，对应旧入口的测试上下文，后续图的测试节点过滤应单独实现。

全部依赖先只读发现真实 workspace 根和项目 Cargo 版本，再将项目锁文件解析移到
独立临时目录。已有 Cargo.lock 保留原始约束，无锁 workspace 也不在项目内生成锁。
Cargo 1.82 以下使用 `--locked`，已有有效锁仍可解析；需要生成或更新锁文件时返回
明确的只读错误。1.82..1.94 使用公共 `MetadataCommand` 和 `CargoWorkspace::new`
补齐完整依赖模型，避免固定上游给已有锁自动追加第二个 `--lockfile-path`。该窄分支
复用已加载模型的工具链、sysroot、cfg、环境与原始锁约束；有效 target 从 Cargo 自己
合并后的配置读取，保留 features 和 target 过滤，任何失败均不回退 no-deps。
实际 1.81/1.94/1.96 的锁文件和 workspace 成员入口已有本机聚焦证据，尚非双平台验收。

原生项目 metadata、sysroot metadata、目标数据、目标 cfg 和源码加载错误都向调用者
返回。核心沿原生 crate 和 block DefMap 检查实际模块声明，覆盖函数、关联项、闭包、
常量及签名中的模块；实际读取失败或已有模块文件被漏载均返回错误，真正缺失的候选
仍保留为项目诊断。源码身份由原生解析确定，合法 `include_bytes!` 二进制资源可以加载。
VFS 文件身份也可以属于合法二进制资源，不保证存在可查询的 FileText。固定上游解码
失败会漏掉 FileText，原生文本、AST 或导航查询随后可能 panic。加载和每次公共语义
查询各有一个完整事务异常边界，位置校验的原文读取也在查询事务中。加载故障保留诊断
并结束该次加载、丢弃数据库；查询故障返回明确 Err，原生取消保留其错误类型。
完整图调用方遇到任一查询 Err 必须终止该次分析并丢弃项目，不能重试或继续拼接部分图；
完整图与协议路径已固定该义务，并验证公共 Host 对忙碌 helper 及实际 Cargo 子进程的取消回收。
异常边界用于真实事务隔离，不逐文件探测文本可用性。
候选检查只读重读路径，不更新数据库或替换原文。上游源码与 loader 保持固定版本。
调用者必须在加载期间保持输入不变；此校验不提供对并发改写的原子保证。加载完成后
查询始终使用原始不可变快照。上游可能返回的 no-deps 回退不能当作成功。当前只有 macOS ARM64
聚焦证据，详见 [迁移验证](../../docs/research/existing-native-helper-migration-validation.md)。

## 图与协议边界

`projection::analyze` 迁入既有 syntax、test_code、symbols、引用与身份规则，所有原文、
AST、字节位置和导航使用同一不可变数据库。没有 LSP、独立语法重解析或第二图构造器。
helper 发送候选 DTO 与配置/选集/节点出处 evidence，最终校验由公共 Host 完成。
真实物理别名归并为一组节点，所有词法选中路径均保留 coverage 与 origin。

显式 `rust-project.json` / `.rust-project.json` 优先，随后查找祖先 Cargo 清单或直接
子目录的独立 Cargo 项目。多项目通过固定上游公开 crate 图、VFS 与 source-root API
合入一次数据库；不递归发现并合并任意配置。非空 standalone 选集保留旧入口的加载失败，
空选集仍可成功。JSON 图不生成虚构 Cargo 容器；声明 crate 根读失败时整图终止。
升级固定 rust-analyzer revision 时须复核多项目装载、`EditionedFileId` 解析和旧 Cargo
metadata 补偿点，不复制上游 loader 或配置合并器。

stdout 正常终结仅包含 ready 与唯一 completed 消息。helper 在 ready 前复用共享
进程设施建立自身专属域（Unix 保留/建立自己的进程组，Windows 使用自身嵌套 Job），
独立控制线程持续等待严格 cancel 或 stdin EOF。阻塞的原生加载不能协作取消时，
helper 主动终止自身及全部后代；不增加 worker 或脱离 Host 的隔离域。此分支不发送
cancelled 帧，公共 Host 在已记录取消且没有已观察的真实失败时映射为取消，并保留
既有整树退出屏障。正常 error/ok 仍发送唯一 completed 并 exit 0，不等待 stdin EOF。
helper 不维持后台会话、watcher、构建脚本或宏进程。`scratch_root`
仅为 Host 协议授权字段，临时锁使用系统 tempfile 生命周期并在正常退出时清理；强制终止
与既有临时目录一样可能留下无项目副作用的系统临时文件。真实支持包优化构建的耗时、
峰值内存、Windows 及安装/Review 验收仍由后续 Rust 集成阶段完成。

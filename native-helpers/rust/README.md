# Rust 原生分析核心

此独立 crate 将 rust-analyzer `9074e9b4c6bc31d7986ccf4e22d18af70e5508da`
的 `project-model`、`load-cargo`、`ide`、`syntax` 和 `vfs` 直接链接到同一个程序。
目前提供可复用的只读 Cargo workspace 核心和聚焦测试，尚未提供 helper 协议、完整
CodeGraph、应用入口或支持包。

## 构建与验证

helper 使用固定 Rust 1.98.0；被分析项目仍使用其实际工具链，二者不得混用。
首次准备 helper 构建工具链可以运行 `rustup toolchain install 1.98.0 --profile minimal`，
不要修改 rustup 默认版本，也不要为被分析项目自动安装版本或 rust-src。

从仓库根目录执行，所有 Cargo 入口均经过仓库锁和 sccache wrapper：

```sh
scripts/cargo-agent.sh +1.98.0 check --manifest-path native-helpers/rust/Cargo.toml --lib --locked
scripts/cargo-agent.sh +1.98.0 test --manifest-path native-helpers/rust/Cargo.toml --lib --locked -- --test-threads=1
```

使用项目支持的 Node 版本运行 wrapper。测试中的标准库/全部依赖案例需要本机项目
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
禁止 build scripts、过程宏服务和 rustup 自动安装。`test_cfg` 明确控制语义 cfg；
默认启用，对应旧入口的测试上下文，后续图的测试节点过滤应单独实现。

全部依赖先只读发现真实 workspace 根和项目 Cargo 版本，再将项目锁文件解析移到
独立临时目录。已有 Cargo.lock 保留原始约束，无锁 workspace 也不在项目内生成锁。
Cargo 1.82 以下使用 `--locked`，已有有效锁仍可解析；需要生成或更新锁文件时返回
明确的只读错误。旧版本实际运行证据留待兼容验收，不能据此宣布已验收。

原生项目 metadata、sysroot metadata、目标数据、目标 cfg 和 crate 根文件加载错误
都向调用者返回。上游可能返回的 no-deps 回退不能当作成功。当前只有 macOS ARM64
聚焦证据，详见 [迁移验证](../../docs/research/existing-native-helper-migration-validation.md)。

# Python native analysis core

独立 Rust 库固定 ty `0.0.84` 对应的 Ruff revision
`11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a`。生产依赖的数据库、AST 和 codegen
全部同源；registry Ruff `0.0.15` 只作为旧行为的测试对照。

`PythonProject::load(ProjectInput)` 接收单个已归一化项目：`repository` 是磁盘仓库根，
`project_root` 和有序 `source_roots` 是干净的仓库相对目录（空路径表示仓库根），
`python_minor` 显式为 7–14。配置解析、PEP 723 版本冲突、跨项目冲突、选集和测试过滤
由后续应用适配器复用既有规则，本核心不会发现配置或解释器。

加载只读遍历项目根及显式源码根，校验每个可达路径的真实仓库边界；`.git`、`.venv`
被排除，可达安装环境、逃逸/损坏链接、读取与 UTF-8/编码声明错误明确失败。
源码和目录被冻结到内存 System；内部链接按词法相对路径保留独立模块身份。
调用方必须在加载期间固定输入，快照复制并不是应对并发磁盘写入的原子事务。
加载后原文、AST 和导航使用同一个固定快照。

项目元数据显式设置源码根、版本、全平台、空 extra-paths、内存空 Python prefix 和
`UseUv::Off`。System 没有用户环境、配置目录、缓存、可写能力、命令执行器或可执行文件
查找。内嵌 typeshed 来自固定上游 revision，标准声明不读取用户 typeshed 或 site-packages。
上游依赖的构建脚本只在开发构建时运行，与分析时执行被分析项目没有关系。

`source` 返回原文，`parse` 返回同源 AST 并拒绝恢复/版本错误，`definitions` 接受 UTF-8
字节位置并保留原生全部定义候选和完整/焦点范围。候选保留来源原文；仓库来源为词法相对
路径，标准来源为内嵌 `stdlib/...`。明确的标准函数/类声明另有模块/词法限定名，例如
`builtins` / `str.upper`，其他声明保留原始来源与范围供后续适配。

导航可能同时包含已知目标和缺依赖重绑定，多个 overload 可以映射到相同的标准符号。
这些候选不删除、不合并，也不表示确定的运行时目标。完整绑定、关系、外部身份和图投影
仍须后续 Task 6 验收。此库尚未接公共 Host、协议、ProcessTree、Review 或支持包。

在仓库根执行（所有 Cargo 命令经全局锁与 sccache wrapper）：

```sh
scripts/cargo-agent.sh +1.98.0 check --manifest-path native-helpers/python/Cargo.toml --lib --locked
scripts/cargo-agent.sh +1.98.0 test --manifest-path native-helpers/python/Cargo.toml --lib --locked -- --nocapture
scripts/cargo-agent.sh +1.98.0 clippy --manifest-path native-helpers/python/Cargo.toml --all-targets --locked -- -D warnings
scripts/cargo-agent.sh +1.98.0 fmt --manifest-path native-helpers/python/Cargo.toml -- --check
```

本轮证据与未验收范围见 `docs/research/existing-native-helper-migration-validation.md`。

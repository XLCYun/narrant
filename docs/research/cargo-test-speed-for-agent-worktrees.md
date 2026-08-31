# Narrant 多 worktree 场景下的 Rust/Cargo 测试与编译提速调研

日期：2026-08-21

## 结论

调研时最值得优化的不是 Rust 测试函数本身，而是多个 Agent 各自冷启动 Cargo、各自生成完整 `target`，并在 8 核/8 GiB 机器上同时运行默认并发的编译任务。当时工作区的观测结果是：8 个 worktree 的 `src-tauri/target` 合计约 32.9 GiB，磁盘只剩约 4.4 GiB；本机尚未安装 `sccache`、`cargo-nextest` 或 `mold`。

2026-08-31 已采纳“独立 incremental target + 共享 sccache + 仓库级 Cargo 锁”的组合，见 [ADR-0027](../adr/0027-use-isolated-incremental-targets-with-sccache.md) 和 [Agent 构建指南](../agents/rust-build-cache.md)。下文的机器状态和 A/B 数字保留为采纳前基线。

建议按下面的顺序处理：

1. 先修复 `build.rs` 对不存在 `.env.local` 的无条件监听。当前它会让每条 Cargo 命令都重新编译 Narrant 自身，这是收益最直接、风险最低的改进。
2. 再建立跨 worktree 的 Cargo 进程互斥、清理已完成 worktree 的 target，并把“每个小修都跑全量 Cargo”改成分层测试。单个 Cargo 进程保留默认 jobs；实测 `jobs=2` 明显更慢。
3. 保持每个并发 worktree 使用独立 `CARGO_TARGET_DIR`，不要把几个有不同源码的 worktree 指向同一个 target；用 `sccache` 共享可复用的 `rustc` 编译结果，而不是共享 Cargo 的 target 状态。
4. 用 `cargo check`、`cargo test --lib <filter>` 或具体 `--test` 做实现期验证；每个纵向 Issue 完成后只做一次全量 `cargo test --all-targets` 加 `cargo test --doc`。
5. 在基准数据证明测试运行时间占主要比例后，再评估 `cargo-nextest`；在 Cargo timing 证明链接占主要比例后，再单独试验 macOS 的 `ld64.lld`。profiles 和 `codegen-units` 暂时不应作为第一批改动。

这些影响区间是针对本仓库的待验证估计，不是工具的性能承诺。

## 当前仓库与机器的事实

- `src-tauri/Cargo.toml` 是一个单 package，包含 `rlib`、`staticlib`、`cdylib` 和 `bin` 目标，测试主要以内嵌 Rust 模块测试存在；当前 `cargo metadata` 没有发现独立 integration-test target。见 [`src-tauri/Cargo.toml`](../../src-tauri/Cargo.toml)。
- 当前主工作树加 7 个 linked worktree，共 8 个 Rust target 目录，大小约为 32.9 GiB。最大的几个约为 5.9 GiB、6.1 GiB 和 4.8 GiB。
- 当前机器为 Apple Silicon、8 个逻辑 CPU、8 GiB 内存，文件系统可用空间约 4.4 GiB（观测时磁盘使用率约 98%）。
- 仓库没有 `.cargo/config.toml`，没有配置 `CARGO_TARGET_DIR`、`CARGO_BUILD_JOBS`、`RUSTC_WRAPPER` 或 Cargo profile 覆盖。
- [quality-gate.yml](../../.github/workflows/quality-gate.yml) 已使用 `Swatinem/rust-cache@v2`。Rust job 依次执行 `fmt`、`clippy --all-targets`、`cargo test --all-targets` 和 `cargo test --doc`；本地 Agent 流程没有对应的共享编译缓存和并发调度。

## 关键事实与对本项目的含义

### 0. 不存在的 `.env.local` 让 Narrant 每次都被判定为 Dirty

`src-tauri/build.rs` 当前无条件输出
`cargo:rerun-if-changed=../.env.local`，但各 Agent worktree 通常没有这个被忽略的本地文件。
Cargo verbose 输出明确报告：

```text
Dirty narrant ...: the file `../.env.local` is missing
```

Cargo 官方说明，build script 可以用 `cargo::rerun-if-changed=PATH` 缩小重新执行范围；
被监听输入发生变化时，build script 及依赖其输出的 package 会重新构建。
[Cargo build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html#rerun-if-changed)
当前监听一个不存在的路径使 Narrant package 在没有源码变化时仍重复编译。

本机在已经编译过依赖的 `babysit-mode` worktree 中连续测得
`cargo test --no-run` 约 11–13 秒，并且每次都显示 `Compiling narrant`；测试二进制本身
运行约 10 秒。也就是说，当前一次普通聚焦测试可能先支付一笔与修改无关、且与测试
数量无关的 package 重编译成本。

建议只在 `.env.local` 实际存在时输出该文件的 `rerun-if-changed`；不存在时不要监听
这个缺失路径。代价是首次新建 `.env.local` 后需要显式触发一次重新构建，可以通过
开发文档说明执行 `touch src-tauri/build.rs` 或等价命令。另一种方案是把公开的 GitHub
App 本地配置改为运行时读取，但这会扩大产品改动，不适合作为第一步。

### 1. Git worktree 共享 Git 对象，不共享工作目录的 Cargo 输出

Git 官方文档把 linked worktree 描述为同一仓库附加的多个 working tree，并明确区分每个 worktree 的工作目录和共享的 repository metadata；refs 等 Git 数据可共享，但每个 worktree 有自己的 checkout 状态。[git-worktree 文档](https://git-scm.com/docs/git-worktree)

Cargo 默认把 target 和 build 输出放在 workspace 根目录的 `target` 中；`CARGO_TARGET_DIR`、`build.target-dir` 或 `--target-dir` 可以改变位置。[Cargo build cache](https://doc.rust-lang.org/cargo/reference/build-cache.html) 因此不同 worktree 的默认路径天然不同，当前 32.9 GiB 的重复依赖产物是预期结果。

把多个不同源码的 worktree 直接指向同一个 target 不能当作安全的共享缓存。Cargo 的 target-dir 锁定设计曾明确以整个 target 目录为粒度，相关 Cargo 跟踪 issue 讨论了并行实例的锁竞争和更细粒度锁定；这至少意味着并发共享会串行化，且 Cargo 没有为“多个 divergent checkout 同时写同一套输出”提供可依赖的工作流契约。[Cargo #4282](https://github.com/rust-lang/cargo/issues/4282) 这是根据 Cargo 的锁设计和不同源码的 fingerprint/artifact 路径作出的工程判断，不是说所有共享 target 都必然损坏。

结论：

- 同一 worktree 内的 `check → test → clippy` 可以复用同一 target。
- 不同 Agent 的不同分支应保持 target 隔离。
- 共享的应该是内容寻址的 `rustc` 编译缓存（例如 sccache），而不是 Cargo 的 fingerprint、incremental 和 build-script 状态目录。

### 2. Cargo 默认并发对本机内存不友好

Cargo 的 `build.jobs` 默认值是逻辑 CPU 数，可通过 `CARGO_BUILD_JOBS` 或 `--jobs` 覆盖。[Cargo configuration](https://doc.rust-lang.org/cargo/reference/config.html#build) 当前机器默认上限约为 8 个 compiler process；如果 3 个 Agent 同时跑 Cargo，理论上会形成约 24 个 compiler process，还会叠加 rustc 的 codegen、链接器和 Tauri build script。

本仓库 A/B 表明，不应设置固定的 `CARGO_BUILD_JOBS=2`：重建 Narrant package 时默认 jobs 为 12.36 秒，jobs=2 为 25.66 秒，后者慢约 108%。最终方案是用仓库公共 Git 目录中的文件锁确保多个 linked worktree 不同时启动 Cargo，而获得锁的单个 Cargo 进程继续使用默认并行度。这同时避免多进程争抢资源，又保留单次编译吞吐。

### 3. 当前 test profile 已经是偏快速的默认值

Cargo 的 `test` profile 继承 `dev` profile；默认 `dev` 使用 `opt-level=0`、`lto=false`、`incremental=true`、`codegen-units=256`。增量编译会把额外信息写入 target 以加快后续重编译；更多 codegen units 有利于编译并行，但可能牺牲运行时性能。[Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)

因此：

- 不要先改成 release、开启 LTO 或降低 `codegen-units`；这些很可能增加测试编译时间。
- 反复在同一个 worktree 编辑时保留 incremental 有价值。
- 一次性 CI 冷构建如果使用 sccache，可以关闭 incremental，因为 sccache 官方 Rust 文档说明增量编译结果不能缓存；但仍会反复修改和验证的 Agent worktree 与 main 一样保留 incremental。[sccache Rust caveats](https://github.com/mozilla/sccache/blob/main/docs/Rust.md)

### 4. sccache 适合本场景，但不能替代 target 隔离

sccache 官方支持通过 `RUSTC_WRAPPER=sccache` 或 Cargo 的 `build.rustc-wrapper` 接入 Rust 编译，并提供 `sccache --show-stats` 查看命中情况。[sccache usage](https://github.com/mozilla/sccache/blob/main/README.md) 它缓存的是单次 `rustc` invocation，适合让多个 worktree 复用完全相同的第三方依赖编译。

需要注意三个本项目相关的限制：

- 默认绝对路径必须一致；sccache 提供 `SCCACHE_BASEDIRS`，可以把多个 checkout 根目录从 cache key 中剥离，从而提高 worktree 之间的命中率。[sccache path normalization](https://github.com/mozilla/sccache/blob/main/README.md#normalizing-paths-with-sccache_basedirs)
- 增量编译不能缓存，需用 `CARGO_INCREMENTAL=0` 测试 sccache 模式。[sccache Rust caveats](https://github.com/mozilla/sccache/blob/main/docs/Rust.md)
- 调用系统 linker 的 `bin`、`dylib`、`cdylib` 和 `proc-macro` crate 不能缓存。Narrant 的 `bin`/`cdylib`/`staticlib` 目标意味着 sccache 主要帮助依赖和可缓存的 rustc 阶段，最终链接仍会发生。[sccache Rust caveats](https://github.com/mozilla/sccache/blob/main/README.md#known-caveats)

sccache 本地磁盘缓存默认上限为 10 GiB，且本地 storage 同一时刻只应运行一个 sccache server；在当前只剩 4.4 GiB 的机器上必须显式设较小上限并监控磁盘。[sccache local storage](https://github.com/mozilla/sccache/blob/main/docs/Local.md)

预期：对第二个及后续冷 worktree，第三方依赖阶段可能获得中等到较高收益（粗估 20–60% 的冷编译 wall time，必须用命中率验证）；workspace 自己改动的 crate 和最终链接收益较小。若 `SCCACHE_BASEDIRS`、Rust 版本或 flags 不稳定，收益会明显下降。

### 5. cargo-nextest 主要优化测试运行，不优化编译

nextest 通过每个测试一个进程来并行运行测试，并且可以控制测试线程数与独立的 build jobs；官方文档也说明其 process-per-test 是与 `cargo test` 的关键区别。[nextest running tests](https://nexte.st/docs/running/) 这会改善测试隔离、失败定位和并行运行，但编译阶段仍由 Cargo 构建测试二进制。

nextest 还支持把 test run 分片、将一次构建归档后在多个机器上复用。[partitioning](https://nexte.st/docs/ci-features/partitioning/) [archiving](https://nexte.st/docs/ci-features/archiving/) 这对未来 CI 矩阵有价值，但对当前只有约百个以内嵌模块测试、主要痛点是冷编译和磁盘的本地 Agent 流程，收益尚未证明。nextest 不运行 doctest，当前 CI 仍需单独保留 `cargo test --doc`。

建议先做 warm-target A/B；只有测试运行时间占总耗时的明显比例且 nextest 至少稳定快 20% 时，才把它加入仓库命令。需要检查现有测试是否依赖同一进程内的全局状态、工作目录或固定端口。

### 6. 目标选择比反复全量运行更直接

`cargo check` 会编译依赖和包，但跳过最终 code generation，因此比 `cargo build` 快；Cargo 官方明确建议它用于只检查编译错误的场景。[cargo check](https://doc.rust-lang.org/cargo/commands/cargo-check.html)

`cargo test` 默认会构建 library、必要的 binary、examples、unit/integration tests 和 library doctests；`--all-targets` 等价于 `--lib --bins --tests --benches --examples`，而 `--doc` 只运行 library documentation tests。[cargo test target selection](https://doc.rust-lang.org/cargo/commands/cargo-test.html#target-selection)

结合 Narrant 当前单 package 的目标形态，建议：

- 实现期间：`cargo check --lib` 或 `cargo check --all-targets --profile=test`。
- 修改某个模块：`cargo test --lib <module-or-test-filter>`。
- 修改独立 integration target 后：只运行对应的 `cargo test --test <name>`。
- Issue 完成时：一次 `cargo test --all-targets --locked`，再一次 `cargo test --doc --locked`。

过滤 test name 仍会构建所需测试二进制，所以对首次冷编译的帮助有限；它主要减少 warm target 上的测试运行时间。

### 7. linker 只应在 timing 证明它是瓶颈后尝试

Cargo 支持按 target 配置 linker，rustc 也支持 `ld64.lld` 这一 Darwin LLD flavor。[Cargo linker config](https://doc.rust-lang.org/cargo/reference/config.html#targettriplelinker) [rustc linker flavors](https://doc.rust-lang.org/rustc/codegen-options/index.html#linker-flavor)

LLVM 的 Mach-O LLD 文档称 `ld64.lld` 是 Apple `ld64` 的替代品，并指出大型程序链接可能超过 `ld64` 两倍速度，但它需要单独安装、验证 Tauri/native framework、签名和发布产物兼容性。[LLVM Mach-O LLD](https://lld.llvm.org/MachO/)

`mold` 的官方文档定位是 GNU ELF linker 的替代品，适合 Linux CI 的实验，不适合作为当前 macOS 开发机的默认方案。[mold](https://github.com/rui314/mold) 对 Narrant，先用 `cargo test --timings` 看链接占比；只有链接超过总时间约 15–20% 才值得做单独分支实验，并保持默认 Apple linker 作为回退。

### 8. CI 已有缓存，但应区分 CI 缓存和本地 Agent 缓存

当前使用的 [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache) 会缓存 Cargo registry 和依赖构建产物，清理 incremental 和非依赖 workspace artifacts，并基于 Rust 环境、lockfile 等生成 key；它还主动设置 `CARGO_INCREMENTAL=0`。这对 CI 冷启动有帮助，但不能解决本地多个 worktree 各自写 target 的磁盘问题，也不应把 CI cache 直接等同于安全的并发 target。

CI 方面可做的低风险调整是给 Rust 命令加 `--locked`，并记录 cache hit、编译时长和 target 大小；不建议在没有基准前把 clippy、test、doc 粗暴合并或拆成更多会重复编译的 job。

## 按优先级的建议

| 优先级 | 建议 | 预期影响（待测） | 努力/风险 | 具体做法 |
| --- | --- | --- | --- | --- |
| P0 | 修复缺失 `.env.local` 的 Cargo invalidation | 高：消除每条 Cargo 命令约 11–13 秒的固定重编译 | 极低/低 | `build.rs` 仅在文件存在时输出 `rerun-if-changed`; 增加“连续两次 no-run 第二次为 Fresh”的回归检查 |
| P0 | Agent Cargo 跨 worktree 互斥 | 高：减少 swap、失败重编译和等待，同时保留单次构建吞吐 | 低/低 | 统一通过 `scripts/cargo-agent.sh` 获取 common git dir 中的锁；不要固定 `CARGO_BUILD_JOBS` |
| P0 | worktree target 生命周期 | 高：立即释放约 33 GiB 级别空间，避免磁盘满 | 低/中 | 只清理已完成且已交接的 worktree target；worktree 移除时同步移除 target；给临时 target 加 TTL。不要共享不同分支的 target |
| P0 | 分层测试门禁 | 高：实现期单轮从全量降到秒级/小分钟级 | 低/低 | `check → targeted test → Issue 完成一次 full targets + doc`；审查小修只跑受影响模块测试 |
| P1 | 独立 target + sccache | 中到高：后续冷 worktree 的依赖编译可复用 | 中/中 | 已采纳：开发 worktree 保留 incremental；仓库脚本设置 `RUSTC_WRAPPER=sccache` 和 2 GiB 上限，观察 `sccache --show-stats` |
| P1 | CI 记录和复用缓存指标 | 中：减少 CI 冷启动和定位时间 | 低/低 | 保留 `Swatinem/rust-cache`，加 `--locked`；记录 cache hit、wall time、top compile units 和 target 大小 |
| P2 | cargo-nextest A/B | 低到中：只影响 test runtime | 中/中 | 先安装在用户/CI 环境，不立即改默认命令；warm target 下与 `cargo test --all-targets` 做 3 次中位数比较，保留 doctest lane |
| P2 | ld64.lld 实验 | 低到中：仅在 linker 占比高时 | 中到高/高 | `cargo test --timings` 证明链接瓶颈后，单独配置 Darwin target linker；验证 Tauri debug build、E2E、签名和发布包后再决定 |
| P3 | Cargo profile 微调 | 不确定，当前默认已偏快 | 中/中 | 暂不改 `opt-level`、LTO、debug 或 `codegen-units`；只在 timing 明确指向某 profile 后做局部 profile/环境实验 |

## 推荐的 Agent 测试节奏

在不牺牲最终质量的前提下，建议把质量检查拆成“实现反馈”和“交付门禁”，并把 Cargo 测试当作有限资源调度：

```text
修改 Rust 文件
   │
   ├─ 立即：cargo fmt --check / cargo check --lib       （可多次）
   ├─ 聚焦：cargo test --lib <changed-module-or-filter> （可多次）
   ├─ 审查修复：只重跑受影响测试；不因文案/TS 小修启动 Cargo
   └─ Issue 完成：一次 cargo test --all-targets --locked
                 一次 cargo test --doc --locked
                 一次完整前端/构建门禁
```

主 Agent 通过 `scripts/cargo-agent.sh` 维护仓库级 Cargo semaphore：同一台机器最多一个 Rust 编译；可以并行做文档、前端静态审查和不启动 Cargo 的代码审查。各 worktree 继续使用独立 target。

## 本次 A/B 结果与已采用方案

| 场景 | A | B | 结论 |
| --- | ---: | ---: | --- |
| warm `cargo test --no-run` | 缺失 `.env.local` 被持续判 Dirty，中位数 11.72 秒 | 条件监听后中位数 1.89 秒，最终复验 1.26 秒且显示 `Fresh narrant` | 消除约 84% 的重复等待，采用 |
| Narrant package rebuild | 默认 jobs 12.36 秒 | jobs=2 25.66 秒 | jobs=2 慢约 108%，拒绝 |
| 磁盘 | 已完成 worktree target 重复占用十余 GiB，可用空间约 5.2 GiB | 清理 #74–#77 的可再生成 target 后可用空间 21 GiB | 将 target 清理纳入 issue 收尾 |

最终落地组合是：修复 build-script invalidation；用 `scripts/cargo-agent.sh` 在 linked worktree 间互斥 Cargo；单个 Cargo 保持默认 jobs；各开发 worktree 保留独立 target 和 incremental，并通过 sccache 共享可缓存的 rustc 结果；实现期运行 check/聚焦测试，Issue 交付前再运行一次全量门禁；合并后的 worktree 及时清理 target。

## 建议的基准实验

不要用“感觉快了”决定是否改配置。固定同一 commit、同一 Rust toolchain、同一机器和同一依赖缓存，连续做 3 次并取中位数；冷 run 与 warm run 分开。

### 记录字段

- wall time、user/sys CPU time、最大 RSS：`/usr/bin/time -lp`。
- Cargo 编译关键路径：`cargo test --timings` 产生的 `target/cargo-timings/cargo-timing.html`。[Cargo timing](https://doc.rust-lang.org/cargo/commands/cargo-test.html#compilation-options)
- target 增长和可用磁盘：`du -sh "$CARGO_TARGET_DIR"`、`df -h .`。
- sccache 命中/未命中/不可缓存原因：`sccache --show-stats`。[sccache statistics](https://github.com/mozilla/sccache/blob/main/README.md#usage)
- nextest 每个 test 的 wall time，以及测试总运行时间。[nextest running tests](https://nexte.st/docs/running/)

### 对照组

1. 当前基线：独立 target、默认 jobs、无 sccache，分别测冷 `cargo test --all-targets --no-run`、warm `cargo test --all-targets` 和 `cargo test --doc`。
2. 并发控制：独立 target、`CARGO_BUILD_JOBS=2`，同样三组命令。
3. sccache：独立 target、`RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0`，先填充缓存，再在另一个 checkout 测冷依赖编译；记录 `SCCACHE_BASEDIRS` 有/无两组。
4. nextest：复用同一 warm target，比较 `cargo test --all-targets` 与 `cargo nextest run`；单独保留 doctest。
5. linker：只有前述 timing 显示链接显著时，比较默认 `ld64` 与 `ld64.lld`，并跑最小 Tauri/E2E 回归。

通过标准建议是：不出现磁盘低于 10 GiB、artifact corruption、随机测试失败或 Tauri 链接/运行回归；在此约束下，优先选择总 wall time 和磁盘占用的帕累托更优方案。

## 暂不建议的方案

- 不把所有 worktree 的 `CARGO_TARGET_DIR` 设为同一个目录来“共享依赖”。这会引入锁等待，且对不同源码的 fingerprint/artifact 复用没有安全契约。
- 不把 `cargo clean` 作为每轮失败后的默认恢复动作；它会删除本 worktree 的增量状态，导致下一轮重新冷编译。只在确认 artifact 已损坏或 worktree 即将结束时使用。
- 不为每个 review comment 或每个很小的 Rust 修改运行全量 `cargo test --all-targets`。
- 不在没有 timing 的情况下切换 linker、开启 LTO 或改全局 profile。
- 不直接把 nextest 的 retry 当作修复 flaky test 的替代品；nextest 官方会把重试成功标记为 flaky，应该保留可见性。[nextest retries](https://nexte.st/docs/features/retries/)

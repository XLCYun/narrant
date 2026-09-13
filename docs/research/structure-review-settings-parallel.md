# 结构改动设置与并行分析：PR31 验证

日期：2026-09-14。分支：`codex/rust-analyzer-repository-scope`。

## 本次改动

应用设置新增「结构改动」分区，按语言选择配置。目前提供 Rust 的三档依赖范围：

1. 仅仓库代码，默认禁用标准库和第三方依赖。
2. 仓库代码 + 标准库，禁用第三方依赖。
3. 仓库代码 + 标准库 + 第三方依赖。

设置自动保存，下一次分析生效。已有结果可点击「重新分析」。两侧在运行前固定同一份
选项，中途修改设置不会改变正在运行的会话。失败保留已保存的选项。

拿到 Snapshot lease 后同时执行 before 和 after：before 签出后立即分析，after
直接分析现有 head。before 的签出可以与 after 的项目加载、语义查询重叠；两侧分析
也可重叠。没有再次签出 head。一侧出错、panic 或外部取消会停止另一侧，实际 worker
退出后才允许释放 lease，刷新、关闭不会提前删除正在被读取的目录。

## 标准库开启、第三方依赖关闭的耗时

测量采用真实应用入口，包含 lease 获取、before 签出、两个 rust-analyzer 会话、
语义查询、图构建、graph diff、投影、head 校验和 before 清理。不包含打开 Review
时首次准备 head、客户端编译或前端渲染；head 在六次运行中始终复用。

| 样本 | 串行 | 并行 |
| --- | ---: | ---: |
| 1 | 11.684 s | 7.559 s |
| 2 | 11.332 s | 7.541 s |
| 3 | 10.815 s | 7.669 s |
| 中位数 | **11.332 s** | **7.559 s** |

同配置下，中位数减少 **3.773 秒 / 33.3%**，约 **1.50 倍**速度。
串行样本中 before 分析耗时 5.15–5.95 秒，after 为 4.97–5.03 秒；
并行样本中两侧各耗时 6.87–6.98 秒、6.25–6.33 秒，发生了资源竞争，因此不会减半。
并行时 after 在总计时开始后 0.26–0.30 秒进入分析，before 在 0.39–0.44 秒进入分析，
证实 after 没有等待 before 签出完成。各侧分析计时包括自身的项目加载和语义查询，
本轮不另外插入生产阶段日志。

每次返回 before 115 个节点、after 172 个节点、59 个变更对象。基准逐字段比较六次
完整结果（含节点、引用、包含关系和 diff），全部一致，并非只比较数量。

## 固定输入和环境

- PR31 base：`a14f544a8f698903db911c2b10c8f46ccff631eb`。
- PR31 head：`6c7c55e8a2dd08eedf4eeff12f25a8b1578fdae4`。
- 文件为 `src-tauri/src/lib.rs`、`src-tauri/src/github_discovery.rs`、
  `src-tauri/tests/github_discovery.rs`；不存在于某侧的文件按应用既有规则排除。
- `include_tests=false`，`dependency_scope=standard-library`。
- rust-analyzer `1.97.1 (8bab26f4 2026-07-14)`，macOS arm64，相同本地启动器。
- `cargo.noDeps=true`，`cargo.sysroot="discover"`，Cargo offline；build scripts、
  过程宏、诊断、检查和 cache priming 均关闭。
- 基准用共享对象库的临时 clone 创建独立 Review，head 只建立一次，再交替执行
  串行、并行各三轮。原仓库和已安装应用中的 PR31 工作副本未改动。
- Rust 工具链和 rust-src 已安装，文件缓存不是全冷；测量期间没有启动本任务的
  Cargo 编译。数值是本机有限样本，不能代表首次安装或所有项目的等待时间。

原始 JSON 和日志位于忽略目录 `.tools/repository-scope-perf/` 的
`settings-standard-library-benchmark.json`、`settings-standard-library-benchmark.log`。
之前的[仓库范围实验](rust-analyzer-repository-scope.md)只计分析，不包含本次的工作副本
准备与清理；不能直接用不同统计口径计算并行收益。

## 中间档仍有的语义限制

允许标准库加载能找回部分关系，但当前 rust-analyzer 将 `noDeps` 同样用于标准库
workspace 的 metadata。crate 图里虽然有 std/core/alloc，彼此的依赖边可能不完整，
因此允许加载不等于恢复全部标准库语义。另一个限制是第三方包装类型的行为仍然未知。

之前对相同 PR 的独立图审计得到：

| 范围 | before 图内关系 | after 图内关系 |
| --- | ---: | ---: |
| 仅仓库 | 32 | 51 |
| 仓库 + 标准库 | 33 | 52 |
| 完整依赖上下文 | 46 | 67 |

中间档恢复了两侧的 `open_saved_branch_review → insert_with_id`：恢复 sysroot 后，
`?` 表达式内的调用可以被解析。剩余 15 条不同的关系中，13 条涉及第三方
`tauri::State<T>` 的解引用，另外两条涉及 Option/Some、for 循环和 From trait。
这些数值是先前独立语义审计，不是本轮并行计时中额外执行的实验。

配置含义见 [rust-analyzer 配置文档](https://rust-analyzer.github.io/book/configuration.html)，
sysroot 对 `noDeps` 的复用见 [sysroot 加载实现](https://github.com/rust-lang/rust-analyzer/blob/master/crates/project-model/src/sysroot.rs)。
三档都保留适配器的仓库源码边界；已解析图外目标和未解析目标继续不显示。
完整档也不会开启 build scripts 和过程宏，因此不承诺恢复宏生成代码的全部关联。

## 复现

先通过仓库锁编译测试，再直接执行测试二进制。不能用持有全局 Cargo 锁的 `cargo test`
直接运行真实 LSP 基准，否则服务端的 Cargo 子进程会等待同一把锁。

```sh
mkdir -p .tools/repository-scope-perf
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --lib --no-run \
  --message-format=json > .tools/repository-scope-perf/settings-test-build.jsonl
export NARRANT_STRUCTURE_BENCHMARK_REPOSITORY=/absolute/path/to/narrant
export NARRANT_STRUCTURE_BENCHMARK_BASE=a14f544a8f698903db911c2b10c8f46ccff631eb
export NARRANT_STRUCTURE_BENCHMARK_HEAD=6c7c55e8a2dd08eedf4eeff12f25a8b1578fdae4
export NARRANT_RUST_ANALYZER=/absolute/path/to/rust-analyzer
export NARRANT_STRUCTURE_BENCHMARK_SCOPE=standard-library
export NARRANT_STRUCTURE_BENCHMARK_OUTPUT=.tools/repository-scope-perf/settings-standard-library-benchmark.json
python3 - <<'PY'
import json
import subprocess
from pathlib import Path

rows = (json.loads(line) for line in Path(
    '.tools/repository-scope-perf/settings-test-build.jsonl').read_text().splitlines()
    if line.startswith('{'))
binary = next(row['executable'] for row in rows
    if row.get('reason') == 'compiler-artifact' and row.get('executable')
    and row.get('profile', {}).get('test') and row['target']['name'] == 'narrant_lib')
subprocess.run([binary,
    'structure_review::application::tests::benchmark::compares_sequential_and_parallel_repository_analysis',
    '--exact', '--ignored', '--nocapture', '--test-threads=1'], check=True)
PY
```

## 页面验收

通用 Preview Harness 提供 `settings/structure-review`、`structure-review-en`、
`structure-review-error`、`structure-review-saving` 场景。实测 1440×1000 浅色、
1024×900 深色、390×844 英文长文案及 800×900 的保存中、保存失败状态，并检查语言
下拉展开、选项切换、可访问 DOM 和实际边界，无新增横向溢出或文字与单选框重叠。
保存失败保留旧值，保存中禁用重复提交；浏览器未报告运行异常。

应用内浏览器打开请求目前处于 queued；实际自动化渲染检查使用独立 Chrome CDP
会话完成，未声称已完成原生应用内浏览器验收。截图与 DOM 证据在 `.tools/evidence/`，
检查后恢复 viewport。

结构图的「重新分析」按钮另外检查了 1440、800、390 像素窗口，能点击且没有新增
横向溢出。一次性浏览器驱动脚本已移除，独立验证浏览器已关闭。

## 自动化验证

- Rust 全量测试：583 通过，13 忽略；其中上述真实基准另外显式运行并通过。
- 前端全量测试：43 个文件、369 项通过，包含设置默认值、三档保存、重进页面、
  读取失败重试、保存失败回退、保存中禁用及 typed command 契约。
- Rust 聚焦测试覆盖两侧同时进入分析、传递相同配置、单侧失败/panic 通知另一侧、
  取消/刷新/关闭/丢弃请求时保留 lease 直到两个 worker 退出，以及 before 的精确回收。
- Rust `check --lib`、rustfmt 和 all-target Clippy（warnings denied）通过。
- ESLint、TypeScript 和前端生产构建通过；构建保留既有的大 chunk 提示。
- 没有构建安装包，没有触发 quality-gate。

# Narrant：系统 Git CLI 与 git2-rs/libgit2 的实现选择

日期：2026-07-25

## 结论

Narrant 应把 **Git CLI 作为 Repository Operations 的唯一生产实现**，但这里的 CLI
既可以是用户安装的 Git，也可以将来改成 Narrant 随应用分发的 Core Git。不要把
git2-rs 与 Git CLI 混合成两条长期生产路径。

原因不是「CLI 更容易调用」，而是 Narrant 已经把产品约束收敛为：用户选择一个已
clone 的真实仓库，Narrant 要在这个仓库上 fetch、创建 linked worktree，并让 LSP
看到与用户 Git 环境一致的 checkout。此时兼容 Git 仓库及其传输、配置和扩展，比
进程内 API 的类型便利更重要。

CLI 并不意味着解析面向人的自然语言。Narrant 所需的大部分写操作只需要检查退出码；
读操作可以使用 Git 明确定义的 porcelain/plumbing、NUL 分隔和自定义 format。

## 成熟工具采用什么

公开的一手资料显示，主流桌面开发工具更常围绕 **Core Git 可执行程序**构建，而不是
直接嵌入 libgit2：

- VS Code 官方文档明确写明它使用机器上安装的 Git；Git 功能要求 Git 2.0 或更新版本
  [VS Code Source Control 文档](https://code.visualstudio.com/docs/sourcecontrol/overview#_prerequisites)；
  其源码最终以 `cp.spawn(this.path, args, options)` 执行 Git，并设置 locale、pager 等环境
  [VS Code Git 实现](https://github.com/microsoft/vscode/blob/main/extensions/git/src/git.ts)。
- IntelliJ IDEA 提供 “Path to Git executable” 设置，并支持选择 WSL2 中的 Git
  [IntelliJ IDEA Git 设置文档](https://www.jetbrains.com/help/idea/settings-version-control-git.html)。
- GitHub Desktop 不是 libgit2 客户端。其 `dugite-native` 项目构建一个针对应用脚本
  调用裁剪和优化的 Core Git；它特意移除翻译，以保持脚本输出稳定
  [desktop/dugite-native](https://github.com/desktop/dugite-native)；Dugite 明确说明自己使用
  Core Git 的 command-line interface，并将结果表示为 exit code/stdout/stderr
  [desktop/dugite](https://github.com/desktop/dugite#readme)。
- Eclipse/JGit 是成熟反例：Eclipse 生态维护了一套纯 Java Git 实现
  [Eclipse JGit 项目](https://projects.eclipse.org/projects/technology.jgit)。
  这说明嵌入式实现可行，但代价是长期维护一个独立 Git 实现及其兼容层，并非免费的
  “不用解析文本”。

因此更准确的行业结论是：**VS Code、JetBrains 和 GitHub Desktop 这类桌面开发工具
都选择运行 Core Git；区别只是使用系统 Git，还是随应用提供受控版本的 Git。**

## CLI 输出是否脆弱

不必解析本地化的人类文本。建议的接口如下：

| Narrant 操作 | Git 调用 | 解析要求 |
|---|---|---|
| 验证仓库、找 common dir | `rev-parse --show-toplevel --git-common-dir` | 每项一个受约束路径 |
| 解析 ref 到 commit | `rev-parse --verify <ref>^{commit}` | 单个 OID |
| 枚举 refs | `for-each-ref --format='...%00'` | NUL 分隔、自定义字段 |
| merge base | `merge-base <base> <head>` | 单个 OID；退出码表达无共同祖先 |
| File Changes 元数据 | `diff --raw -z` 或 `diff --name-status -z` | Git 文档定义的记录 |
| patch/hunks | `diff --patch --no-color` | Git patch 格式；不是自然语言 |
| 创建、锁定、移除 worktree | `worktree add/lock/remove` | 成功路径只看退出码 |
| 检查 worktree | `worktree list --porcelain -z` | 稳定 porcelain |
| fetch | `fetch ...` | 正常成功只看退出码；stderr 仅供用户诊断 |

Git 官方保证 `status --porcelain` 适合脚本使用、跨版本和用户配置保持稳定；`-z` 使路径
保持原始字节并用 NUL 分隔
[git-status](https://git-scm.com/docs/git-status#_porcelain_format_version_2)。
`diff --raw/--name-status -z` 同样避免文件名引用和换行歧义
[git-diff](https://git-scm.com/docs/git-diff#Documentation/git-diff.txt--z)。
`for-each-ref --format` 支持字段插值和 `%00`，可自行定义稳定记录
[git-for-each-ref](https://git-scm.com/docs/git-for-each-ref)。
`worktree list --porcelain -z` 的格式也被承诺跨版本和配置稳定
[git-worktree](https://git-scm.com/docs/git-worktree#Documentation/git-worktree.txt---porcelain)。

真正仍可能变化的是失败时的 stderr。它不应驱动业务逻辑：业务只看退出码和后置条件；
stderr 原样作为诊断信息展示/记录，必要时再按错误类别补充已知映射。进程启动时应设置
稳定 locale（例如 `LC_ALL=C`）主要用于日志一致性，而不是依靠英文句子做正确性判断。

## 兼容性比较

### worktree 与仓库格式

Core Git 定义 linked worktree 的共享/私有数据、`add --detach`、lock、remove、prune 和
repair 完整生命周期
[git-worktree](https://git-scm.com/docs/git-worktree.html)。
libgit2 近年持续补齐兼容性；例如 1.8 才集中加入 per-worktree refs、worktree-specific
config 和更接近 `git worktree add` 的行为
[libgit2 1.8 发布讨论](https://github.com/libgit2/libgit2/discussions/6775)，而新的
relative worktree 扩展仍曾需要单独跟进
[libgit2 issue 7210](https://github.com/libgit2/libgit2/issues/7210)。

Narrant 当前 `git2 = 0.19` 虽然暴露 worktree API，但 API 存在不等于与用户安装的
Core Git 所创建的所有仓库格式和新扩展同步。Narrant 的目标恰好是操作用户的现有仓库，
所以应让该仓库的原生实现负责它。

### fetch、认证与 SSH

系统 Git 天然沿用用户已验证的 transport 环境：remote URL、Git config、credential
helper、Git Credential Manager、SSH 可执行程序、agent、`~/.ssh/config`、企业代理和
证书配置。

libgit2/git2-rs 能通过 callback 提供凭据，但应用必须自行编排这些能力；这不是单纯的
`fetch()` 调用。对 Narrant 而言，这会复制用户已经在终端和 IDE 中配置好的认证集成。
系统 Git 的风险是桌面进程不能允许不可控的交互提示；应使用受控 `GIT_ASKPASS` 或
credential helper，并支持取消与超时，绝不把 token 放到 argv 或持久化 Git config。

### LFS、submodule、attributes 与 filters

Git LFS 是独立 Git 扩展，通过 pointer 与实际大文件内容的替换工作
[Git LFS 官方站点](https://git-lfs.com/)；Git attributes 可以在 checkout 时使用
filter，在 diff 时使用自定义 diff/textconv 驱动
[gitattributes](https://git-scm.com/docs/gitattributes)。

使用 Core Git checkout 可以继承用户已有的 LFS/filter 行为；libgit2 路径需要分别确认
并集成这些外部能力。Submodule 仍应被当作显式能力：创建主仓库 worktree 不等于自动
初始化所有 submodule，Narrant 第一阶段可以显示 gitlink 而不递归拉取，之后再按权限和
磁盘策略增加初始化。

### 对 Narrant 最重要的含义

如果 `repo/` 的目标是提供“用户在该 commit 上正常 checkout 后会得到的项目”，Core Git
兼容路径更符合目标。如果目标只是读取 blob/tree 并自己生成文件，libgit2 更有吸引力；
但那不是当前已经确认的 linked-worktree + LSP 产品模型。

## 打包、跨平台与版本

### 系统 Git

优点：

- 应用包更小，不承担 libgit2、libssh2/OpenSSL 等 native 依赖的构建矩阵；
- macOS、Windows、Linux 上与用户终端/IDE 的认证和仓库行为一致；
- Git 安全更新由用户的 Git 分发渠道提供。

代价：

- 首次添加仓库时必须定位可执行文件、运行 `git --version` 并做 capability probe；
- 用户版本可能不同，必须定义最低版本和明确的升级错误；
- Windows 路径、编码、进程取消和无控制台执行都要纳入测试；
- 用户 Git 配置会影响行为，调用必须显式关闭颜色、pager 和可变展示选项。

不要仅凭版本号推断全部能力。建议在 ADR 落地时，根据实际使用的最老选项确定最低版本，
并在启动时执行小型 capability probes。若将来“无需用户安装 Git”成为需求，可像 GitHub
Desktop 一样随应用分发 Core Git，而不改变 `SystemGitAdapter` 上层接口。

### bundled libgit2

优点：

- 进程内结构化 API、错误类型和取消控制更直接；
- Git 行为版本固定，用户无需安装 Git；
- 对纯对象读取、图遍历和定制 diff 可避免子进程开销。

代价：

- Narrant 自己承担 native library 的三平台打包、安全升级和 feature 选择；
- `vendored-libgit2` 只固定了实现，不会自动获得用户 credential/LFS/filter/SSH 生态；
- 用户仓库采用新的 Core Git extension 时，应用可能打不开或行为不同；
- 与 Core Git 的 rename、attributes、worktree 和 checkout 细节差异会形成长期兼容测试面。

对一次 Review 而言，fetch、worktree 和 diff 的成本远高于启动几个短生命周期 Git
子进程，因此子进程开销不是当前决策的关键因素。

## 安全与测试要求

无论选择哪种实现，checkout 不可信 PR 都不是纯读取：它会把文件写入磁盘。使用 CLI 时：

1. Rust 必须直接使用 `std::process::Command`/Tokio process 和参数数组，不经过 shell。
2. 所有 revision 先解析为完整 OID；路径参数使用 `--` 或固定参数位置，不能把用户输入
   当 revspec/option 拼接。
3. 设置 `GIT_PAGER=cat`、`GIT_OPTIONAL_LOCKS=0`（仅只读操作）、禁用颜色，并隔离可变
   locale。
4. 为进程提供超时、取消、stdout/stderr 上限；fetch progress 不能无限占用内存。
5. 创建 worktree 时用一个 Narrant 管理的空 hooks 目录覆盖 `core.hooksPath`，避免
   checkout 触发用户仓库的 `post-checkout` hook。Git 官方列出 `post-checkout` 会在
   `git worktree add` 后运行
   [githooks](https://git-scm.com/docs/githooks#_post_checkout)。
6. 明确决定 filter 策略：保留 filters 可获得 LFS/真实 checkout，但外部 filter 也是
   代码执行边界。第一阶段可设置 `GIT_LFS_SKIP_SMUDGE=1`，并把缺失 LFS 内容作为降级
   状态；不能笼统声称工作区“完全只读、不会执行外部程序”。
7. 用包含空格、换行、非 UTF-8（Unix）、Unicode、rename、submodule、shallow clone、
   SSH/HTTPS remote、linked worktree 和崩溃恢复的真实临时仓库做跨平台集成测试。

## 是否混合使用

不建议按操作随意混合：

```text
fetch/worktree 用 CLI + merge-base/diff 用 libgit2
```

这种方案表面减少解析，实际上引入两套配置、attributes、rename detection、仓库格式和
错误语义。CLI fetch 完成后 libgit2 读取对象还会增加刷新/并发边界。尤其 Narrant 要与
GitHub three-dot diff 和 worktree 中的代码保持一致，单一实现更容易建立不变量。

可以接受的“混合”只有清晰的架构边界：

- GitHub API：PR 发现、身份、标题、描述、评论以及当前 base/head SHA；
- Core Git adapter：本地 refs、fetch、merge-base、diff、worktree；
- 普通文件系统/LSP：读取已经物化的 head 工作区。

若未来性能分析证明某个**纯只读、无配置歧义**的热点值得用 git2-rs 优化，应以相同
fixture 做差分兼容测试，并把它作为缓存/优化，而不是第二个语义来源。

## 对 ADR 0003 的建议

废止“为了不要求安装 Git 而嵌入 libgit2”的决定，新增 ADR：

> Narrant 的 Repository Operations 使用 Core Git 可执行程序。首版使用用户安装的
> Git；adapter 不承诺该可执行文件永远来自系统，未来允许分发受控 Core Git。

这既回答了当前问题，也避免把实现名写死成 `SystemGitAdapter`。更准确的接口名可以是
`GitRepositoryOperations`，首个实现为 `CoreGitProcessAdapter`。

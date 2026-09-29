# Python 结构分析实现验收

本记录对应 [规格 0023](../specs/0023-python-language-analyzer.md)、Issues #189–#192 和
[PR #188](https://github.com/XLCYun/narrant/pull/188)。固定工具为 Ruff 0.16.9 对应 crates 0.0.15、
ty 0.0.84、Python 支持包 1.0.0；本地宿主 macOS ARM64、Rust 1.97.1。

## 已执行的行为验证

- `python_analyzer` 的公开 `LanguageAnalyzer`/CodeGraph 接口：有序父内容、子对象修改隔离、
  类/实例字段去重、property 三角色、真实与伪 overload、类型别名、嵌套调用和函数值引用。
- 真实 ty：仓库 Node、未选中文件 Repository External、固定标准库 Dependency External、
  缺依赖/别名重绑定/配置冲突 Unresolved；测试排除不会经关系重新显露目标。
- Python 3.7–3.14 每个 minor 的类/函数、仓库调用、`len`、`str.upper`；旧目标不被提升到 3.10。
  `python-platform=all` 下 `pathlib.Path.exists` 返回多个平台声明，保留 Unresolved。
- `.pyi` overload 没有伪造实现，`.py/.pyi` 保留不同 ID。两者并存的 fixture 中 ty 导航到 `.py`；
  根据实际声明证据建边，不强制优先 stub 或实现。未知装饰器不升级为 property/overload。
- 继承 property 的写入不创建子类字段；未知基类或已存在继承成员无法确认新字段归属时保守保留方法源码。
- 中文、非 BMP emoji、UTF-8 BOM，LF/CRLF/单独 CR 坐标；错误编码、版本冲突、协议错误为硬失败。
- 三个不同项目的源码根/版本各自独立；选择范围和临时 checkout 变化不改变已有对象 ID。
- 子进程中毒化 HOME/USERPROFILE/XDG/APPDATA、Python/Conda 环境变量，以及仓库自定义 typeshed、
  Python prefix、外部 extra-paths 和 `.venv`；`poison.fetch` 未解析、标准库 `len` 正常导航。
- 逃逸符号链接在启动前拒绝；可达的 Python 安装环境无法建立源码边界时明确失败。
- 超时、取消、调用方 panic、错误坐标会退出并回收进程；不会返回成功的部分图。
- 支持包损坏/缺入口/不可执行被拒绝；安装失败可重试，持有租约不能删除，重启后可复用完整安装。
- 真实安装的包经生产下载/校验/解压入口提供 ty，再执行真实 LSP 和分析；不借助用户 Python。
- 三种语言的 before/after 共用两份 Git 工作区，删除 `.py/.pyi` 只分析存在侧，全部测试过滤无需准备包；
  Python 失败等待 Rust、TS peer 停止后回收 before。

## 本地复现与计数

```sh
pnpm build:language-packs --id python --native --output .tools/language-support-pack-artifacts
node scripts/verify-python-analyzer.mjs .tools/language-support-pack-artifacts
```

当前 macOS 验收：分析器 27 项、支持包 3 项、应用 4 项均通过，包括显式启用的真实服务测试。
普通 CI 的 Rust 测试不默认下载/启动这些工具；两平台支持包 workflow 会显式执行原生验收。

性能 fixture 为 3 项目、24 文件、600 方法。两轮均创建独立受控会话，第二轮仅受系统文件缓存影响，
不跨分析保留语言服务器或项目状态。本机一轮约 2.64 秒、重复一轮约 1.74 秒；同批 native 取消约
157 毫秒完成。数据来自测试负载并发运行，只用于定位量级，不代表用户项目性能保证。
单独复测两轮约 1.81 秒和 1.33 秒；每项目 633 次实际定义查询、8 次缓存命中，包含装饰器和标准库声明校验。
适配器记录这些统计到 debug tracing；同一文档/字节位置不重复查询。

## 界面验收

使用 `pnpm dev:preview` 的通用 Harness，在应用内浏览器检查 1440×1000、900×800 窗口。
`Review 阅读器 / Python 类、访问器与删除声明` 覆盖长名称、Python Monaco 原文差异、图内调用、
`.pyi` 删除及语言选择弹层；External/Unresolved 保持隐藏。侧栏宽 320 px，内部编辑器宽 296 px，
窄窗口无页面横向溢出。支持包场景覆盖 Python 下载、长错误、重试和设置删除/占用状态。
Harness 仅使用代表性数据，真实后端另由上述 Git/原生工具测试验收。

## 合入与发布门槛

- Windows x64 原生构建、LSP、图提取、环境隔离和 junction 验收由 PR 的独立 `Language support packs`
  workflow 执行；跨平台打包成功不能替代 Windows 原生运行。当前等待该 PR 检查结果。
- 全量 Rust、前端测试、脚本测试、Lint 和生产构建完成后在 PR 记录结果。
- 用户要求先审阅实现，未经确认不合入 PR。不手动触发 quality-gate。
- 支持包产物尚未发布到生产 OSS；合入后按[发布文档](../language-support-packs.md)原生验证、
  保护环境授权和公开回读成功，才能发布引用该版本的应用。

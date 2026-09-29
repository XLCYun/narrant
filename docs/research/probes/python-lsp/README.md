# Python LSP 研究探针

对应 [Python 结构分析调研](../../python-structure-analysis.md)。这是限定场景的服务行为记录，
不属于生产适配器或集成验收套件。2026-09-29 在 macOS arm64、Node 24.18.0、
Python 3.14.6 上运行；Python 只用于运行 LSP 客户端，没有执行 fixture 源码。

固定 Pyright 1.1.414、ty 0.0.84。下载 URL、官方摘要与脚本摘要见
[tool-versions.json](tool-versions.json)。Pyright npm tarball 按 `dist.integrity` SHA-512
核验；ty 官方 release binary 按 asset SHA-256 核验，没有执行 npm install 或项目安装脚本。

两种服务各运行两个 documentSymbol 请求及 14 个 definition 查询；原始响应和返回源码范围
保存在 [observations-2026-09-29.json](observations-2026-09-29.json)。该文件省略大量诊断通知，
保留 initialize、documentSymbol、各次 definition 响应、目标源码、shutdown 和退出记录。
原始完整协议和 stderr 的临时目录也保留在其中，临时文件将来可能被系统清理。

## 复现

脚本是当日实际使用版本，按自身位置创建 `fixture`、日志、受控解释器和缓存。
请复制到单独临时目录运行，以免将生成物写入仓库。示例从仓库根执行：

```sh
probe_dir=$(mktemp -d /tmp/narrant-python-probe.XXXXXX)
cp docs/research/probes/python-lsp/probe.py docs/research/probes/python-lsp/ty-probe.py docs/research/probes/python-lsp/tool-versions.json "$probe_dir/"
```

按该目录的 `tool-versions.json` 下载并验证两个官方归档：Pyright 解压到 `pyright/`，
入口应为 `pyright/package/langserver.index.js`；ty 解压到 `ty/`，入口应为
`ty/ty-aarch64-apple-darwin/ty`。只有上述固定 macOS arm64 工具经过本次验证。
随后使用与记录一致的 Node/Python 运行：

```sh
python3 "$probe_dir/probe.py"
python3 "$probe_dir/ty-probe.py"
```

受控解释器只记录参数并退出 1；服务正常结束会收到 shutdown/exit，脚本对退出设置有界等待。
两脚本按顺序运行并重用同一固定 fixture；同目录并发运行未验证。
脚本主要采集证据，返回位置需查看 `targets` 的文件、行和 `selected`，不能只把脚本退出码
或非空响应当成目标正确的证明。历史反例记录在主调研文档。

## 验证范围与限制

- 同一源码 fixture 覆盖相对导入、自动 src layout、简单保持原函数的装饰器、实例方法/属性、
  函数别名、顺序重绑定、同签名函数的未知 bool 分支、`.pyi/.py` 并存、中文/emoji 坐标及标准库。
- 两者配置并未完全统一：Pyright 日志采用 Python 3.14；ty 从 fixture 的 `requires-python`
  采用 Python 3.11。这些基础语法在二者目标版本都合法，本轮不据此作跨版本完整性或准确率排名。
- Pyright 配置显式指向受控解释器。ty 使用默认设置，清理继承的 Python 环境变量，PATH 只含
  受控 python/python3；最终用 `XDG_CACHE_HOME` 将 stub 缓存定位到探针目录。
- ty 没有 workspace/didChangeConfiguration notification handler；最终脚本不发送该通知。
  本轮没有验证内联配置、`untrustedWorkspace`、受控空环境或限制文件读取的生产方案。
- 没有调用 references、typeDefinition、callHierarchy 或 typeHierarchy；它们的实现存在性来自
  独立源码核查，不属于这 28 个 definition 查询的实测结论。
- 没有验证复杂装饰器/descriptor/metaclass、跨侧缓存隔离、语法层提取、图差异、真实项目依赖、
  大仓性能、Windows 生命周期或应用取消链路。
- 最终两个服务退出码均为 0，进程残留检查为空。Pyright 的解释器日志累计自 3 次探索会话，
  不能把其中 18 次调用当成最后一次会话的调用次数。ty 的两次探索没有受控解释器调用记录；
  这不能证明所有配置永不调用外部命令。

初次 ty 探索曾将 typeshed 写入默认 `~/.cache/ty`。最终已验证独立缓存；默认缓存可能共享，
本轮没有删除。归档保存的是修正后未知 bool 参数的 fixture，未保留初始可被常量折叠的样例结果。

## 第二轮：环境边界

用户确认仓库 + 固定标准库后，另外进行了 [8 个 ty 环境边界会话](boundary/README.md)，
包含 64 个 definition 查询、污染项目配置、空环境、嵌套项目和外向符号链接反例。
这批数据固定 Python 3.14 / darwin，保留在独立目录，不覆盖第一轮结果。

# ty 0.0.84 分析环境边界探针

结论：对本次固定 fixture，工具控制的空 configurationFile、清洗后的 inline environment、显式空 Python prefix、受控 XDG_CONFIG_HOME 能稳定保留仓库源码和固定 bundled stdlib 的定义解析，并排除项目配置中的 extra-paths 与用户 venv。**这些配置不构成文件读取沙箱。仓库内指向外部的符号链接仍被跟随，返回 URI 词法路径甚至仍在仓库内。**

## 固定工具与过程

- 使用上一轮已按 GitHub asset SHA-256 校验的 ty 0.0.84 macOS arm64 binary；本轮 binary/archive 摘要记录于 [manifest.json](manifest.json)，官方归档 URL 与摘要见上级 [tool-versions.json](../tool-versions.json)。
- 8 个独立顺序会话，源码均由 boundary_probe.py 创建，不执行 fixture、不安装包、不访问真实用户 venv、不运行 Cargo、不改仓库，也没有改写上一轮证据。
- 每次 initialize 均带 untrustedWorkspace=true、experimental.useUv=off；configuration 同时作为 initializationOptions 及 workspace/configuration(section=ty) 的返回值，全部交换过程留在各会话 protocol.jsonl。
- 清除继承的 TY_、PYTHON、UV_、VIRTUAL_ENV、CONDA、PIP 前缀环境变量，再明确设置 PATH 到仅含受控 python/python3 记录脚本的目录，PYTHONNOUSERSITE=1；XDG_CONFIG_HOME 指向工具空目录，XDG_CACHE_HOME 指向工具缓存。全部会话日志确认 Python 3.14、darwin。未重新定义 HOME。
- 受控解释器若被启动只记录参数并 exit 1；本轮无调用记录。进程均 shutdown/exit 正常返回 0，生命周期与空残留检查见 observations.json 中的 lifecycle。

## 固定配置形状

```json
{
  "initializationOptions": {
    "untrustedWorkspace": true,
    "experimental": {"useUv": "off"},
    "diagnosticMode": "off",
    "configurationFile": "/tool/control/empty-ty.toml",
    "configuration": {
      "environment": {
        "root": ["/repo", "/repo/src"],
        "extra-paths": [],
        "python": "/tool/control/empty-python-prefix",
        "python-version": "3.14",
        "python-platform": "darwin"
      }
    }
  }
}
```

empty-ty.toml 内容为空；empty-python-prefix/lib/python3.14/site-packages 是工具建立的空目录。嵌套场景 root 另含明确验证过的 /repo/services/app/src。所有路径采用绝对路径。本探针每次仍保留 rootUri/workspaceFolders 为真实 fixture 仓库，进程 cwd 也是该仓库。客户端配置回调返回同一 settings，不能返回未经清洗的用户编辑器设置。

## 8 会话结果

“无外部声明”在本工具实际返回本地 from-import 的名称锚点，并非 null；需要读取返回位置的源代码才能区分。

| 会话 | 根/src 源码 | 外部 extra-paths | 伪 venv | 嵌套包 | bundled Path/len |
|---|---|---|---|---|---|
| 01 默认发现污染 ty.toml | 正确声明 | 外部声明 | venv 声明 | 未放置 | 正确声明 |
| 02 默认发现污染 pyproject | 正确声明 | 外部声明 | venv 声明 | 未放置 | 正确声明 |
| 03 只用 inline extra-paths=[]、空 prefix | 正确声明 | **仍有外部声明** | 无外部声明 | 未放置 | 正确声明 |
| 04 空配置文件＋清洗配置，原 ty.toml 污染 | 正确声明 | 无外部声明 | 无外部声明 | 未放置 | 正确声明 |
| 05 空配置文件＋清洗配置，原 pyproject 污染 | 正确声明 | 无外部声明 | 无外部声明 | 未放置 | 正确声明 |
| 06 根 workspace，子项目 pyproject 污染 | 正确声明 | 无外部声明 | 根目录自动发现 venv | 未解析到子包 | 正确声明 |
| 07 固定配置＋显式子项目 src root | 正确声明 | 无外部声明 | 无外部声明 | 正确声明 | 正确声明 |
| 08 固定配置＋仓库内外向 symlink | 正确声明 | 无外部声明 | 无外部声明 | 未放置 | 正确声明 |

第 08 会话的 linked_sentinel.py 是仓库内符号链接，指向 outside-source/linked_sentinel.py。definition 成功定位 EXTERNAL_LINK；targetUri 是仓库内 linked_sentinel.py，canonical_path 是外部真实文件。词法路径前缀检查不能建立仓库边界。若产品承诺分析上下文不含这些文件，应先规范化路径、排除逃逸链接，并在最终目标与依赖映射时再次校验；如果承诺绝不读取外部文件，还需要独立的文件系统隔离机制，本配置探针不能证明那一要求。

## 精确边界与未测项

- untrustedWorkspace=true 在默认污染配置下仍解析 extra-paths 和 venv；它的“禁止外部命令”不能解释成“忽略环境/忽略文件配置/限制文件读取”。
- inline extra-paths=[] 没有清除本次 ty.toml 的原外部路径。必须保留空 configurationFile 的覆盖措施，或验证其他等价机制。
- unresolved import 的 definition 非空：返回本地 import 名称锚点。集成不能将“非空”视作解析成功，也不能仅靠 URI 在仓库内就认定取得真正的定义。
- 第 06 会话是以 monorepo 根作为唯一 workspace/rootUri；嵌套 services/app/pyproject 未自动作为独立项目采用。它不能证明所有 monorepo 布局都如此。第 07 会话证明显式根列表可解析本例子项目源码。
- 空配置覆盖与 roots 选择经过本 fixture 的定义结果验证；未进行文件系统访问追踪，所以“无外部定义”不证明“未读取外部文件”。第 08 会话已明确提供读取外部源码的反例。
- 没有毒化真实或工具 XDG 用户配置；本轮只是将 XDG_CONFIG_HOME 固定为空目录。没有测试父目录配置、PEP 723、工作区动态改变、文件 watcher、不同 Python/平台、系统 prefix 其他布局、Windows/macOS 沙箱或语言服务器升级。
- 伪 venv 的 pyvenv.cfg home 指向不存在目录；默认会话日志警告无法跳转 stdlib 真实实现，但仍成功解析其 site-packages sentinel 和 bundled typeshed。这是有意避免接触真实解释器的 fixture。

## 归档与复现

- [boundary_probe.py](boundary_probe.py)：当日实际运行的中文注释脚本，内嵌固定 fixture。
- [observations.json](observations.json)：8 个会话的配置、initialize/definition 原始响应、返回源文与真实路径、结果评价和生命周期。省略诊断通知与 stderr，保留全部 64 个 definition 查询。
- [manifest.json](manifest.json)：固定工具和脚本摘要，以及完整原始协议/日志所在的临时目录；临时文件将来可能被系统清理。

脚本按自身目录生成 fixture、伪环境、控制文件、缓存与日志。按[上级复现说明](../README.md)准备固定 ty 后，把本目录复制到相同临时工作目录：

```sh
cp -R docs/research/probes/python-lsp/boundary "$probe_dir/"
python3 "$probe_dir/boundary/boundary_probe.py"
```

要求 ty 入口位于 `$probe_dir/ty/ty-aarch64-apple-darwin/ty`；不要在仓库文档目录直接运行。
脚本重跑会生成完整 results.json、sessions 协议和 lifecycle.json；归档的 observations.json/evaluation 是本次观察，不会自动重写。

# Issue #155A: Backend Application Language Freeze

## Objective

让 Babysit 后端在每次 Line Explanation 生成开始时，从 Application Settings 的同一份快照中取得并冻结 Application Language；请求调用方不能覆盖语言。

## Scope

- 从 Rust `BabysitFileRequest` 与公开 Tauri `explain_review_file_change` command 参数删除 `locale`。
- 在 provider/model/effort 解析所在的生成启动边界读取一次 Application Settings，并把该快照中的 Application Language 映射到 provider request locale。
- 继续把冻结后的 locale 纳入完成缓存 identity，保证中文与英文结果隔离。
- 设置读取失败时以生成启动失败返回，不使用前端值或隐式默认值。
- 在途生成保留启动时冻结的语言；后续设置更新不取消、不重试、不改变该次 provider request。
- 主动 `regenerate` 建立新运行并读取最新设置语言。
- 更新受影响 Rust fixture 与测试，但不改变 provider-independent Line Explanation schema、prompt、修复协议或国际化范围。

## Public Acceptance Tests

- Tauri command/application seam 证明序列化请求不再接受 `locale`，并能从中英文 Application Settings 生成对应 provider locale。
- 可控在途 provider 证明运行期间更新设置后，当前运行仍使用旧语言；下一次主动 regenerate 使用新语言。
- 缓存测试证明不同 Application Language 不会错误复用旧语言结果。
- 缺失/不可读设置以设置持久化错误失败。

## Constraints

- Rust 命令仅通过 `scripts/cargo-agent.sh` 运行。
- 新增或修改的方法、结构体提供易读中文注释；私有测试位于模块 `tests/` 目录。
- 不运行全量测试；由雄蜂合并所有分片后统一运行一次。
- 不触发 quality-gate。

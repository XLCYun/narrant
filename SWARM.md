## Cargo 是蜂群共享的单槽资源

- 所有 worktree 的 Rust 验证必须通过 `scripts/cargo-agent.sh`。
- 一个 agent 同时只能拥有一个未完成的 Cargo 命令；静默时必须轮询原 session，禁止追加探测命令。
- 雄蜂负责汇总本 Issue 的验证请求、消除重复，并串行授权工蜂使用 Cargo。
- 蜂王在多个 Rust Issue 并行时明确全局优先级。
- 将等待全局锁与测试挂起分开判断；只有确认命令已获锁并进入 test binary 后，才能诊断测试 hang。
- 普通排队、慢编译或测试失败不得用 `cargo clean` 处理。
- 编辑可以并行，Cargo 验证应在分片完成后尽量合并执行。
- 每个 Issue 应设置验证预算；审查小修尽量合并为一次模块测试，且命令必须报告实际命中的测试数，`0 passed` 不算有效验证。

## 分片与交付

- 两个 Issue 若共同修改同一 trait、注册表或核心模块，应先由一个前置 Issue 冻结共享 seam，再并行开发 adapter；依赖关系同时按代码所有权建模。
- 工蜂交付必须包含可合并 commit、干净工作树、实际命中的验证结果和残余风险。只交 red test、无提交或持续空转时，雄蜂可以在硬期限后接管原 worktree。
- 有界工具在实现前列出结果条数、单项字节、总扫描预算、UTF-8 边界、非法文本和取消矩阵。
- Provider 契约在实现前列出空/非空认证、effort wire shape、精确工具集合、同会话 repair、HTTP/tool/late-response 取消和 completed-cache 不写入矩阵。
- 协议 adapter 完成后保留独立的 application-level 集成 Issue，证明跨协议一致性、真实工具循环、恢复 UI、取消和缓存行为。

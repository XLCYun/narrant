## Cargo 是蜂群共享的单槽资源

- 所有 worktree 的 Rust 验证必须通过 `scripts/cargo-agent.sh`。
- 一个 agent 同时只能拥有一个未完成的 Cargo 命令；静默时必须轮询原 session，禁止追加探测命令。
- 雄蜂负责汇总本 Issue 的验证请求、消除重复，并串行授权工蜂使用 Cargo。
- 蜂王在多个 Rust Issue 并行时明确全局优先级。
- 将等待全局锁与测试挂起分开判断；只有确认命令已获锁并进入 test binary 后，才能诊断测试 hang。
- 普通排队、慢编译或测试失败不得用 `cargo clean` 处理。
- 编辑可以并行，Cargo 验证应在分片完成后尽量合并执行。

# 统一公开仓库迁移验证

决策见 [ADR-0036](../adr/0036-use-one-public-github-repository-for-distribution.md)。
本次代码变更基准为 `44b65e43b53f79eaa37ee210946fced1facb050b`，保留此前 Native Helper 迁移、
下载任务身份修复、Windows NSIS 启动器识别和 E2E 修复。

## 已完成检查

- README/英文版和两张截图取自原公开说明的固定提交 `596f88407e69d22083a4ca9d58c20370915145bf`，
  当前链接指向 `XLCYun/narrant`；原开发说明移入 `docs/development.md`。
- 根许可证为 Apache-2.0；第三方许可文件保持原许可，不被项目许可证覆盖。
- 所有工作流使用 GitHub-hosted macOS ARM64 / Windows x64 镜像；actionlint 语法与表达式检查通过。
- 应用 Release、updater manifest、语言支持包统一使用同仓库 GitHub Releases；语言包使用 prerelease，
  不占用应用 Latest。移除跨仓库 token、OSS/OIDC 发布和共享桌面的独立升级包构建链。
- 正式包验收拒绝本地和 self-hosted 环境；保留升级后模拟项目、主题与语言设置的持久化检查。
- 使用 Gitleaks 对全部本地 Git refs 的历史、当前源码及当时可下载的 133 个 Actions run 日志扫描。
  历史扫描 733 个有改动提交；当前源码 916 个文件；Actions 日志共约 47.2 MB，无不可下载项。
  三项均未发现规则命中的密钥。这是自动检测结果，不等同于对所有敏感信息的穷尽证明。
- 本地最终验证：Rust 624 项、前端 426 项、脚本 218 项通过；脚本另有 2 项平台跳过。
  类型检查、Lint、actionlint 和 macOS 原生窗口 helper 的 Swift 编译通过。
- GitHub 已将 `XLCYun/narrant` 设为公开，匿名 API 读取确认 `private: false`。
- GitHub-hosted [运行 38928118116](https://github.com/XLCYun/narrant/actions/runs/38928118116)
  验证代码提交 `296418a1ae5b47145f3a8000b9c04e4c3d547a77`；macOS ARM64 与 Windows x64
  的支持包构建/Python 集成、完整 Native Analyzer 共四项任务全部通过，整个 run 为 success。
  后续仅补传升级测试的 `RUNNER_TEMP` 并调整说明，相关 driver/TLS 17 项测试通过；
  不改变该运行验收的语言实现和 CI 命令。未手动触发 quality-gate。
- macOS runner 的 GitHub 注册、LaunchAgent、专用账户/组及 `/Users/Shared/narrant-ci`
  已清理。Windows 于 2026-10-17 重启后核验：服务、进程、计划任务、专属用户组、旧服务账户
  配置、`C:\ci`、`ci-setup`、专属 Python 与下载缓存均已清理，并补清失效 PATH 条目和卸载日志。
  GitHub runner 数量再次核验为 0，双平台退役已完成。个人 Narrant 与共用开发工具保留。
- Mac 后续补清约 277 MiB 的旧 CI 调查文件、下载缓存和测试工具；开发中的 worktree
  `target` 与共用 sccache 仍按 AGENTS.md 保留。系统管理的三个 WebKit 测试空目录无缓存数据。

## Standards

独立审查发现缺少中文注释及三处环境筛选重复两项问题。已补注释并共用一个环境筛选函数，
复核无剩余问题。

## Spec

独立审查发现升级 driver 丢失 hosted 环境标记，以及候选仅有 PID 就被视为启动成功两项问题。
现已保留必要标记、等待原生窗口就绪，并在候选正常退出后再验证持久化数据；启动未就绪的失败
也会按本次安装路径回收。新增回归检查通过，复核无剩余问题。

审查结论：Standards 原有 2 项、剩余 0 项；Spec 原有 2 项、剩余 0 项。

## 验收边界

- 首个 GitHub Stable 尚未发布，不存在可供 normal 验收的同仓库 previous Stable。
  首版须走受保护 exact tag 的 bootstrap；后续正式包执行双平台真实升级。
- 旧渠道 v1.2.14 用户需手动安装一次新渠道修复版，不能承诺旧二进制自动修复自己。
- 此前 self-hosted 的成功记录不替代迁移后的 GitHub-hosted 验收。
- 本次通过的是语言支持包工作流，不等同于已执行新版全仓 quality-gate 或正式包升级。
- 语言包仍为未发布候选。Python 依赖 `seahash 4.1.0` 原始许可 notice 的来源限制仍记录在
  `native-helpers/python/licenses/sources.json`：发布 crate 声明 MIT，包内提供声明及 MIT 条款，
  精确 GitLab 原始 notice 尚未取回。不能把这项记录表述为完整法律审计。
- 本地 Rust 全量测试曾出现既有 worktree 创建取消清理竞态；该项单独重跑通过，初次失败记录保留。
  最终测试结果与托管 CI run 在 PR 中记录。

正式发布操作见 [发布运维](../application-update-operations.md)；工作流通过不等于已经发布制品。

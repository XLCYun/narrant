# Issue #155B: Frontend Babysit Language Lifecycle

## Objective

移除前端 Babysit 请求对 UI locale 的拥有权，并让语言设置切换对现有 Line Explanations 和在途生成完全无副作用。

## Scope

- 从 TypeScript `BabysitFileRequest`、应用 adapter 输入、typed Tauri command contract 和 preview fixture 删除 `locale`。
- `useBabysitLifecycle` 不再读取 i18n locale、不再把语言放进 request identity，也不再监听语言变化来清除结果或取消请求。
- 语言设置切换保留 completed Line Explanations，不调用 adapter；在途请求继续。
- 现有明确 regenerate 操作仍发送 `generationPolicy: "regenerate"`；语言由后端在新运行开始时决定。
- 更新公开 lifecycle/adapter/transport 测试，覆盖 completed、in-flight、no-auto-generation 与 regenerate 行为。
- 不修改界面翻译资源、硬编码文案或其它设置 UI。

## Public Acceptance Tests

- adapter/transport 测试证明 invoke payload 不包含 `locale`。
- lifecycle 测试证明 i18n language change 不清除 completed state、不取消 in-flight operation、不调用 explain。
- regenerate 测试证明用户操作建立新请求且 generation policy 为 `regenerate`。
- TypeScript 类型检查、受影响前端测试、Lint 与生产构建通过。

## Constraints

- 新增或修改的方法、结构体、类提供易读中文注释。
- 不扩展国际化范围，不触发 quality-gate。

import type { CodeNode, CodeReference, StructureComparison } from "@/structure-review/types";

/** 原文对比预览覆盖长实现、长字符串、注释与字段，避免用规范化文本模拟展示数据。 */
export function structureSourceDiffFixture(): StructureComparison {
  const comparison = structureReviewFixture();
  const longSignature = "pub async fn recover_all_previously_opened_review_workspaces_with_very_long_descriptive_names(&self) -> Result<(), RecoveryError>";
  for (const side of ["before", "after"] as const) {
    comparison[side] = structuredClone(comparison[side]);
    const node = comparison[side].nodes.find((node) => node.id === "long")!;
    const steps = Array.from({ length: 36 }, (_, index) => `    restore_step(${index})?;`).join("\n");
    node.source = `/// 恢复已打开的审核工作区。\n${longSignature} {\n    let message = "保留原始字符串里的空格，长行只在编辑器中自动折行，不改写真实文本";\n${steps}\n    ${side === "before" ? "save_snapshot();" : "validate_snapshot()?;\n    save_snapshot();"}\n    Ok(())\n}`;
    node.code = side === "before" ? "{ restore_step(); save_snapshot(); }" : "{ restore_step(); validate_snapshot()?; save_snapshot(); }";
    comparison[side].nodes.find((node) => node.id === "Review")!.source = `pub struct Review {\n    pub id: ReviewId,\n    pub snapshot: ${side === "before" ? "Option<Snapshot>" : "Snapshot"},\n}`;
  }
  comparison.diff.nodes.push({ id: "long", content: "modified", contains: [], references: [] });
  return comparison;
}

/** 字段类型预览覆盖未变化关系、类型替换和递归字段，目标类型在点击前已参与布局。 */
export function structureFieldTypesFixture(): StructureComparison {
  const typeUse = (id: string): CodeReference => ({ semantic: "code.type_use", target: { kind: "node", id } });
  /** 使用稳定对象身份，让包装类型变化与字段关联变化分别进入 diff。 */
  const node = (id: string, category: CodeNode["category"], extra: Partial<CodeNode> = {}): CodeNode => ({
    id, category, language: "rust", nativeKind: category === "container" ? "module" : category === "type" ? "struct" : category === "value" ? "field" : "fn",
    name: id, signature: null, code: null, source: extra.signature == null ? null : `${extra.signature}${extra.code ? ` ${extra.code}` : ""}`, contains: [], references: [], extensions: {}, ...extra,
  });
  const common = [
    node("github-discovery", "container", { name: "github_discovery", contains: ["Group", "Item", "Repository", "LegacyRepository", "refresh", "store"] }),
    node("Item", "type", { name: "DiscoveredPullRequest", signature: "pub struct DiscoveredPullRequest", contains: ["item-title"] }),
    node("item-title", "value", { name: "title", signature: "pub title: String" }),
    node("Repository", "type", { name: "GitHubRepository", signature: "pub struct GitHubRepository" }),
    node("LegacyRepository", "type", { name: "LegacyRepository", signature: "pub struct LegacyRepository" }),
    node("store", "callable", { name: "store", signature: "fn store()" }),
  ];
  const fields = (after: boolean) => [
    node("Group", "type", { name: "PullRequestGroup", signature: "pub struct PullRequestGroup", contains: ["pull-requests", "repository", "error", ...(after ? ["parent"] : [])] }),
    node("pull-requests", "value", { name: "pull_requests", signature: `pub pull_requests: ${after ? "Option<Vec<DiscoveredPullRequest>>" : "Vec<DiscoveredPullRequest>"}`, references: [typeUse("Item")] }),
    node("repository", "value", { name: "repository", signature: `pub repository: ${after ? "GitHubRepository" : "LegacyRepository"}`, references: [typeUse(after ? "Repository" : "LegacyRepository")] }),
    node("error", "value", { name: "error", signature: `pub error: ${after ? "Option<String>" : "String"}` }),
    node("refresh", "callable", { name: "refresh", signature: "fn refresh()", code: after ? "{ store(); log(); }" : "{ store(); }", references: [{ semantic: "code.calls", target: { kind: "node", id: "store" } }] }),
    ...(after ? [node("parent", "value", { name: "parent", signature: "pub parent: Option<Box<Self>>", references: [typeUse("Group")] })] : []),
  ];
  return {
    ...structureReviewFixture(),
    before: { root: ["github-discovery"], externalSymbols: {}, nodes: [...common, ...fields(false)] },
    after: { root: ["github-discovery"], externalSymbols: {}, nodes: [...common, ...fields(true)] },
    coverage: [{ path: "src/github/discovery.rs", oldPath: null, before: "analyzed", after: "analyzed" }],
    diff: { nodes: [
      { id: "Group", content: null, contains: [{ kind: "added", value: "parent" }], references: [] },
      { id: "pull-requests", content: "modified", contains: [], references: [] },
      { id: "repository", content: "modified", contains: [], references: [{ kind: "removed", value: typeUse("LegacyRepository") }, { kind: "added", value: typeUse("Repository") }] },
      { id: "error", content: "modified", contains: [], references: [] },
      { id: "parent", content: "added", contains: [], references: [{ kind: "added", value: typeUse("Group") }] },
      { id: "refresh", content: "modified", contains: [], references: [] },
    ] },
  };
}

/** 为预览提供开关前后的两种分析结果；测试节点由结果提供，不在真实展示层过滤。 */
export function structureTestCodeFixture(comparison: StructureComparison, includeTests: boolean): StructureComparison {
  if (!includeTests || !comparison.coverage.some((file) => file.before === "analyzed" || file.after === "analyzed")) return comparison;
  const result = { ...structuredClone(comparison), before: structuredClone(comparison.before), after: structuredClone(comparison.after) };
  for (const side of ["before", "after"] as const) {
    const common = { language: "rust", references: [], extensions: {} };
    result[side].root.push("preview-tests");
    result[side].nodes.push(
      { ...common, id: "preview-tests", category: "container", nativeKind: "mod", name: "tests", signature: "#[cfg(test)] mod tests", code: null, source: null, contains: ["preview-test"] },
      { ...common, id: "preview-test", category: "callable", nativeKind: "fn", name: "checks_snapshot_identity", signature: "#[test] fn checks_snapshot_identity()", code: side === "before" ? "{ assert!(snapshot.is_ready()); }" : "{ assert!(snapshot.has_matching_identity()); }", source: `#[test]\nfn checks_snapshot_identity() {\n    assert!(snapshot.${side === "before" ? "is_ready" : "has_matching_identity"}());\n}`, contains: [] },
    );
  }
  result.diff.nodes.push({ id: "preview-test", content: "modified", contains: [], references: [] });
  return result;
}

/** 预览和应用测试共用的代表性图，包含高层对象、导入声明和独立的关系变化。 */
export function structureReviewFixture({ importsOnly = false }: { importsOnly?: boolean } = {}): StructureComparison {
  const call = (id: string): CodeReference => ({ semantic: "code.calls", target: { kind: "node", id } });
  const node = (id: string, category: CodeNode["category"], contains: string[] = [], extra: Partial<CodeNode> = {}): CodeNode => {
    const signature = extra.signature ?? (category === "callable" ? `fn ${extra.name ?? id}()` : `${category} ${extra.name ?? id}`);
    return {
      id, category, contains, language: "rust", nativeKind: category === "container" ? "mod" : category === "type" ? "struct" : category === "extension" ? "impl" : "fn",
      name: id, signature, code: null, source: `${signature}${extra.code ? ` ${extra.code}` : category === "callable" ? " {}" : ""}`, references: [], extensions: {}, ...extra,
    };
  };
  const shared = [
    node("narrant", "container", ["review", "storage"]),
    node("Review", "type", ["review-id", "snapshot"], { signature: "pub struct Review" }),
    node("review-id", "value", [], { name: "id", nativeKind: "field", signature: "pub id: ReviewId" }),
    node("impl", "extension", ["run", "restore"], { name: "impl Review", signature: "impl Review", references: [{ semantic: "code.applies_to", target: { kind: "node", id: "Review" } }] }),
    node("restore", "callable", [], { name: "restore", signature: "pub fn restore(&mut self, state: ReaderState)" }),
    node("keep", "callable", [], { name: "restore_reader_state" }),
    node("storage", "container", ["save", "load", "long", "use-unchanged"]),
    node("use-unchanged", "extension", [], { nativeKind: "use", name: null, signature: "use std::path::Path;" }),
    node("save", "callable", [], { name: "save_snapshot" }),
    node("load", "callable", [], { name: "load_snapshot" }),
    node("long", "callable", [], { name: "recover_all_previously_opened_review_workspaces_with_very_long_descriptive_names", signature: "pub async fn recover_all_previously_opened_review_workspaces_with_very_long_descriptive_names(&self) -> Result<(), RecoveryError>" }),
  ];
  const before: StructureComparison["before"] = { root: ["narrant"], externalSymbols: {}, nodes: [...shared,
    node("snapshot", "value", [], { name: "snapshot", nativeKind: "field", signature: "pub snapshot: Option<Snapshot>" }),
    node("review", "container", ["Review", "impl", "old", "keep", "use-old"]),
    node("use-old", "extension", [], { nativeKind: "use", name: null, signature: "use crate::legacy::load;" }),
    node("old", "callable", [], { name: "load_legacy_state" }),
    node("run", "callable", [], { name: "run_review", signature: "pub async fn run_review(&mut self) -> Result<()>", code: "{ let state = load_legacy_state(); save_snapshot(state); }", source: "/// 加载当前审核并保存快照。\npub async fn run_review(&mut self) -> Result<()> {\n    let state = load_legacy_state();\n    save_snapshot(state);\n    Ok(())\n}", references: [call("old"), call("save"), { semantic: "code.member_of", target: { kind: "node", id: "Review" } }] }),
  ] };
  const unresolved: CodeReference = { semantic: "code.references", target: { kind: "unresolved", name: "generated::on_review_opened" } };
  const external = { semantic: "code.calls", target: { kind: "external", symbol: "rust:registry:serde_json-1.0%2Fsrc%2Fser.rs::fn:to_string" } } satisfies CodeReference;
  const standard = { semantic: "code.calls", target: { kind: "external", symbol: "rust:sysroot:alloc%2Fsrc%2Fsync.rs::impl:Arc%3CT%3E::fn:new" } } satisfies CodeReference;
  const repository = { semantic: "code.calls", target: { kind: "external", symbol: "rust:repo:src%2Fidentity.rs::fn:verify_snapshot" } } satisfies CodeReference;
  const imported: CodeReference = { semantic: "code.imports", target: { kind: "node", id: "load" } };
  const unusedImport = { semantic: "code.imports", target: { kind: "external", symbol: "import-only-target" } } satisfies CodeReference;
  const addedReferences = [call("save"), external, standard, repository, imported];
  const after: StructureComparison["after"] = { root: ["narrant"], externalSymbols: {
    [external.target.symbol]: { name: "serde_json::to_string", scope: "dependency" },
    [standard.target.symbol]: { name: "Arc<T>::new", scope: "dependency" },
    [repository.target.symbol]: { name: "verify_snapshot", scope: "repository" },
    [unusedImport.target.symbol]: { name: "imported_but_unused", scope: "repository" },
  }, nodes: [...shared,
    node("snapshot", "value", [], { name: "snapshot", nativeKind: "field", signature: "pub snapshot: Snapshot" }),
    node("review", "container", ["Review", "impl", "new", "keep", "use-new"]),
    node("use-new", "extension", [], { nativeKind: "use", name: null, signature: "pub use crate::storage::load;", references: [unusedImport] }),
    node("new", "callable", [], { name: "prepare_snapshot", signature: "fn prepare_snapshot(review: Review, options: SnapshotOptions)", code: "{ save_snapshot(Snapshot::new()); }", source: "fn prepare_snapshot(\n    review: Review,\n    options: SnapshotOptions,\n) {\n    save_snapshot(Snapshot::new());\n}", references: addedReferences }),
    node("run", "callable", [], { name: "run_review", signature: "pub async fn run_review(&mut self) -> Result<()>", code: "{ let state = prepare_snapshot(); save_snapshot(state); generated::on_review_opened(); }", source: "/// 准备当前审核并保存快照。\npub async fn run_review(&mut self) -> Result<()> {\n    // 固定本次审核使用的源码版本。\n    let state = prepare_snapshot();\n    save_snapshot(state);\n    generated::on_review_opened();\n    Ok(())\n}", references: [call("new"), call("save"), unresolved, { semantic: "code.member_of", target: { kind: "node", id: "Review" } }] }),
  ] };
  const comparison: StructureComparison = {
    reviewId: "structure-preview", snapshotId: "structure-snapshot", comparisonBase: "1111111", headRevision: "2222222", before, after,
    beforeNotes: [], afterNotes: [],
    diff: { nodes: [
      { id: "review", content: null, contains: [{ kind: "removed", value: "old" }, { kind: "added", value: "new" }, { kind: "removed", value: "use-old" }, { kind: "added", value: "use-new" }], references: [] },
      { id: "use-old", content: "removed", contains: [], references: [] },
      { id: "use-new", content: "added", contains: [], references: [{ kind: "added", value: unusedImport }] },
      { id: "old", content: "removed", contains: [], references: [] },
      { id: "new", content: "added", contains: [], references: addedReferences.map((value) => ({ kind: "added", value })) },
      { id: "run", content: "modified", contains: [], references: [{ kind: "removed", value: call("old") }, { kind: "added", value: call("new") }, { kind: "added", value: unresolved }] },
      { id: "snapshot", content: "modified", contains: [], references: [] },
    ] },
    coverage: [
      { path: "src/review.rs", oldPath: null, before: "analyzed", after: "analyzed" },
      { path: "src/storage.rs", oldPath: null, before: "analyzed", after: "analyzed" },
      { path: "src/structure-changes-view.tsx", oldPath: null, before: "unsupported-language", after: "unsupported-language" },
    ],
  };
  if (!importsOnly) return comparison;
  return {
    ...comparison,
    before: { ...before, root: ["review"], nodes: [node("review", "container", ["use-old"]), before.nodes.find((node) => node.id === "use-old")!] },
    after: { ...after, root: ["review"], nodes: [node("review", "container", ["use-new"]), after.nodes.find((node) => node.id === "use-new")!] },
    diff: { nodes: [
      { id: "review", content: null, contains: [{ kind: "removed", value: "use-old" }, { kind: "added", value: "use-new" }], references: [] },
      ...comparison.diff.nodes.filter((node) => node.id.startsWith("use-")),
    ] },
  };
}

/** 混合语言预览覆盖 TS/JS 共享目标、JSX 关系、同名声明、长源码与逐侧说明。 */
export function structureMixedLanguageFixture(): StructureComparison {
  const comparison = structureSourceDiffFixture();
  const node = (id: string, category: CodeNode["category"], extra: Partial<CodeNode>): CodeNode => ({
    id,
    category,
    language: "typescript",
    nativeKind: category === "container" ? "module" : category === "type" ? "interface" : category === "extension" ? "import" : "function",
    name: id,
    signature: null,
    code: null,
    source: null,
    contains: [],
    references: [],
    extensions: {},
    ...extra,
  });
  const call = (id: string): CodeReference => ({ semantic: "code.calls", target: { kind: "node", id } });
  const jsx = (id: string): CodeReference => ({ semantic: "jsx.uses_component", target: { kind: "node", id } });
  for (const side of ["before", "after"] as const) {
    const graph = comparison[side];
    graph.root.push("typescript-module", "javascript-helper", "merged-plugin-a", "merged-plugin-b");
    graph.nodes.push(
      node("typescript-module", "container", { name: "review-ui", contains: ["typescript-import", "typescript-component", "child-view"] }),
      node("typescript-import", "extension", { name: null, signature: 'import { formatReview } from "./format-review.js";', source: 'import { formatReview } from "./format-review.js";', references: [{ semantic: "code.imports", target: { kind: "node", id: "javascript-helper" } }] }),
      node("javascript-helper", "callable", { language: "javascript", name: "formatReview", signature: "export function formatReview(value)", source: "export function formatReview(value) { return String(value); }" }),
      node("child-view", "callable", { name: "ReviewSummaryCard", signature: "export function ReviewSummaryCard(props: ReviewSummaryProps)", source: "export function ReviewSummaryCard(props: ReviewSummaryProps) { return <strong>{props.title}</strong>; }" }),
      node("typescript-component", "callable", {
        name: "ReviewWorkspaceWithAnExtraordinarilyLongNameThatMustWrapWithoutHidingAnalysisNotes",
        signature: "export function ReviewWorkspaceWithAnExtraordinarilyLongNameThatMustWrapWithoutHidingAnalysisNotes()",
        code: side === "before" ? "{ return <ReviewSummaryCard title={formatReview(review)} />; }" : "{ return <ReviewSummaryCard title={formatReview(review.title)} onOpen={() => openReview(review.id)} />; }",
        source: side === "before"
          ? "/** 保留原生 JSDoc 与 JSX 源码。 */\nexport function ReviewWorkspaceWithAnExtraordinarilyLongNameThatMustWrapWithoutHidingAnalysisNotes() {\n  return <ReviewSummaryCard title={formatReview(review)} />;\n}"
          : "/** 保留原生 JSDoc、长属性与 JSX 源码。 */\nexport function ReviewWorkspaceWithAnExtraordinarilyLongNameThatMustWrapWithoutHidingAnalysisNotes() {\n  return (\n    <ReviewSummaryCard\n      title={formatReview(review.title)}\n      description=\"这是一段用于验证窄窗口详情面板、源码折行和滚动区域的很长 JSX 属性内容\"\n      onOpen={() => openReview(review.id)}\n    />\n  );\n}",
        references: [call("javascript-helper"), jsx("child-view")],
      }),
      node("merged-plugin-a", "type", { name: "PluginRegistry", signature: "export interface PluginRegistry", source: "export interface PluginRegistry { open(): void; }" }),
      node("merged-plugin-b", "type", { name: "PluginRegistry", signature: "export interface PluginRegistry", source: "export interface PluginRegistry { close(): void; }" }),
    );
  }
  comparison.beforeNotes = [{
    code: "typescript.context",
    message: "变更前使用仓库配置与固定运行时声明进行分析。",
    file: null,
    project: "tsconfig.json",
  }];
  comparison.afterNotes = [
    {
      code: "typescript.context",
      message: "变更后使用仓库配置与固定运行时声明进行分析。",
      file: null,
      project: "tsconfig.json",
    },
    {
      code: "typescript.TS2307",
      message: "缺少可选业务依赖的声明；已保留能够确认的本地对象与关系，这段较长说明用于验证弹层内容可以自然换行。",
      file: "src/components/review-workspace-with-a-very-long-file-name.tsx",
      project: "packages/application/tsconfig.json",
    },
  ];
  comparison.coverage.push(
    { path: "src/components/review-workspace.tsx", oldPath: null, before: "analyzed", after: "analyzed" },
    { path: "src/format-review.js", oldPath: null, before: "analyzed", after: "analyzed" },
  );
  comparison.diff.nodes.push({ id: "typescript-component", content: "modified", contains: [], references: [] });
  return comparison;
}

/** 仅图外或未解析关系发生变化，验证空态及显示全部对象后的未变化标记。 */
export function structureHiddenReferencesFixture(): StructureComparison {
  const comparison = structureReviewFixture();
  const callable = comparison.after.nodes.find((node) => node.id === "new")!;
  const module = { ...comparison.after.nodes.find((node) => node.id === "review")!, contains: [callable.id] };
  const references = callable.references.filter((reference) => reference.target.kind !== "node");
  references.push({ semantic: "code.calls", target: { kind: "unresolved", name: "unknown_function" } });
  return {
    ...comparison,
    before: { ...comparison.before, root: [module.id], nodes: [module, { ...callable, references: [] }] },
    after: { ...comparison.after, root: [module.id], nodes: [module, { ...callable, references }] },
    diff: { nodes: [{ id: callable.id, content: null, contains: [], references: references.map((value) => ({ kind: "added", value })) }] },
  };
}

/** 密集类图覆盖多个实现块、长签名和成员之间的调用，仅供预览使用。 */
export function structureClassDiagramFixture(): StructureComparison {
  const comparison = structureReviewFixture();
  comparison.after = structuredClone(comparison.after);
  const type = comparison.after.nodes.find((node) => node.id === "Review")!;
  const implementation = comparison.after.nodes.find((node) => node.id === "impl")!;
  const field = comparison.after.nodes.find((node) => node.id === "review-id")!;
  const method = comparison.after.nodes.find((node) => node.id === "run")!;
  const fields = ["repository", "base_revision", "head_revision", "workspaces"].map((name, index): CodeNode => ({
    ...field, id: `dense-field-${index}`, name, signature: `pub ${name}: ${index === 3 ? "Vec<ReviewWorkspace>" : "String"}`,
    source: `pub ${name}: ${index === 3 ? "Vec<ReviewWorkspace>" : "String"}`,
  }));
  const names = ["open", "close", "refresh", "cancel", "is_current", "recover_all_previously_opened_review_workspaces", "save", "restore_snapshot"];
  const methods = names.map((name, index): CodeNode => {
    const signature = index === 5 ? `pub async fn ${name}(&mut self, options: RecoveryOptions, cancellation: CancellationToken) -> Result<Vec<ReviewWorkspace>, RecoveryError>`
      : `pub fn ${name}(&mut self) -> Result<()>`;
    const body = index === 0 ? "close();" : "Ok(())";
    return { ...method, id: `dense-method-${index}`, name, signature, code: `{ ${body} }`, source: `${signature} {\n    ${body}\n}`,
      references: index === 0 ? [{ semantic: "code.calls", target: { kind: "node", id: "dense-method-1" } }] : [] };
  });
  const contract: CodeNode = { ...type, id: "Persist", name: "Persist", nativeKind: "trait", signature: "pub trait Persist", source: "pub trait Persist {}", contains: [] };
  for (const graph of [comparison.before, comparison.after]) {
    graph.nodes.push(contract);
    const storage = graph.nodes.find((node) => node.id === "storage")!;
    storage.contains = [...storage.contains, contract.id];
  }
  const traitImplementation: CodeNode = { ...implementation, id: "impl-persist", name: "impl Persist for Review", signature: "impl Persist for Review", contains: methods.slice(6).map((node) => node.id),
    source: `impl Persist for Review {\n${methods.slice(6).map((node) => node.source).join("\n\n")}\n}`,
    references: [...implementation.references, { semantic: "code.implements", target: { kind: "node", id: contract.id } }] };
  type.contains = [...type.contains, ...fields.map((node) => node.id)];
  implementation.contains = [...implementation.contains, ...methods.slice(0, 6).map((node) => node.id)];
  const review = comparison.after.nodes.find((node) => node.id === "review")!;
  review.contains = [...review.contains, traitImplementation.id];
  comparison.after.nodes.push(...fields, ...methods, traitImplementation);
  comparison.diff.nodes.push(
    ...[...fields, ...methods, traitImplementation].map((node) => ({ id: node.id, content: "added" as const,
      contains: node.contains.map((value) => ({ kind: "added" as const, value })), references: node.references.map((value) => ({ kind: "added" as const, value })) })),
    { id: "Review", content: null, contains: fields.map((node) => ({ kind: "added", value: node.id })), references: [] },
    { id: "impl", content: null, contains: methods.slice(0, 6).map((node) => ({ kind: "added", value: node.id })), references: [] },
  );
  comparison.diff.nodes.find((node) => node.id === "review")!.contains.push({ kind: "added", value: traitImplementation.id });
  return comparison;
}

/** 库入口和独立集成测试分别归属 crate 根，覆盖 PR #31 中根级函数混在一起的情形。 */
export function structureCrateRootsFixture(): StructureComparison {
  const comparison = structureReviewFixture();
  const template = comparison.after.nodes.find((node) => node.id === "run")!;
  const run: CodeNode = { ...template, id: "crate-run", name: "run", signature: "pub fn run()", code: "{}", source: "pub fn run() {}", contains: [], references: [] };
  const library: CodeNode = { ...run, id: "library-crate", category: "container", nativeKind: "crate", name: "src-tauri/src/lib.rs", signature: null, code: null, contains: [run.id] };
  const testRoot: CodeNode = { ...library, id: "test-crate", name: "src-tauri/tests/github_discovery.rs", contains: ["test-helper", "test-checks", "test-long"] };
  const helper: CodeNode = { ...run, id: "test-helper", name: "repository_with_remotes", signature: "fn repository_with_remotes(remotes: &[(&str, &str)]) -> TempDir", source: "fn repository_with_remotes(remotes: &[(&str, &str)]) -> TempDir {\n    todo!()\n}" };
  const checks: CodeNode = { ...run, id: "test-checks", name: "discovers_pull_requests", signature: "#[test] fn discovers_pull_requests()",
    source: "#[test]\nfn discovers_pull_requests() {\n    let repository = repository_with_remotes(&[]);\n    run();\n}",
    references: [{ semantic: "code.calls", target: { kind: "node", id: run.id } }, { semantic: "code.calls", target: { kind: "node", id: helper.id } }] };
  const long: CodeNode = { ...run, id: "test-long", name: "uses_authenticated_metadata_access_and_keeps_permission_failures_local",
    signature: "#[test] fn uses_authenticated_metadata_access_and_keeps_permission_failures_local()", source: "#[test]\nfn uses_authenticated_metadata_access_and_keeps_permission_failures_local() {}" };
  testRoot.source = [helper.source, checks.source, long.source].join("\n\n");
  const added = [testRoot, helper, checks, long];
  return {
    ...comparison,
    before: { root: [library.id], nodes: [library, run], externalSymbols: {} },
    after: { root: [library.id, testRoot.id], nodes: [{ ...library, source: "pub fn run() {\n    discover_pull_requests();\n}" }, { ...run, code: "{ discover_pull_requests(); }", source: "pub fn run() {\n    discover_pull_requests();\n}" }, ...added], externalSymbols: {} },
    diff: { nodes: [
      { id: run.id, content: "modified", contains: [], references: [] },
      ...added.map((node) => ({ id: node.id, content: "added" as const,
        contains: node.contains.map((value) => ({ kind: "added" as const, value })),
        references: node.references.map((value) => ({ kind: "added" as const, value })) })),
    ] },
    coverage: [
      { path: "src-tauri/src/lib.rs", oldPath: null, before: "analyzed", after: "analyzed" },
      { path: "src-tauri/tests/github_discovery.rs", oldPath: null, before: "absent", after: "analyzed" },
    ],
  };
}

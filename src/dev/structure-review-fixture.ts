import type { CodeNode, CodeReference, StructureComparison } from "@/structure-review/types";

/** 预览和应用测试共用的代表性图，包含高层对象、导入声明和独立的关系变化。 */
export function structureReviewFixture({ importsOnly = false }: { importsOnly?: boolean } = {}): StructureComparison {
  const call = (id: string): CodeReference => ({ semantic: "code.calls", target: { kind: "node", id } });
  const node = (id: string, category: CodeNode["category"], contains: string[] = [], extra: Partial<CodeNode> = {}): CodeNode => ({
    id, category, contains, language: "rust", nativeKind: category === "container" ? "mod" : category === "type" ? "struct" : category === "extension" ? "impl" : "fn",
    name: id, signature: category === "callable" ? `fn ${extra.name ?? id}()` : `${category} ${extra.name ?? id}`, code: null, references: [], extensions: {}, ...extra,
  });
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
    node("run", "callable", [], { name: "run_review", signature: "pub async fn run_review(&mut self) -> Result<()>", code: "{ let state = load_legacy_state(); save_snapshot(state); }", references: [call("old"), call("save"), { semantic: "code.member_of", target: { kind: "node", id: "Review" } }] }),
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
    node("new", "callable", [], { name: "prepare_snapshot", signature: "fn prepare_snapshot(review: Review, options: SnapshotOptions)", code: "{ save_snapshot(Snapshot::new()); }", references: addedReferences }),
    node("run", "callable", [], { name: "run_review", signature: "pub async fn run_review(&mut self) -> Result<()>", code: "{ let state = prepare_snapshot(); save_snapshot(state); generated::on_review_opened(); }", references: [call("new"), call("save"), unresolved, { semantic: "code.member_of", target: { kind: "node", id: "Review" } }] }),
  ] };
  const comparison: StructureComparison = {
    reviewId: "structure-preview", snapshotId: "structure-snapshot", comparisonBase: "1111111", headRevision: "2222222", before, after,
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
  }));
  const names = ["open", "close", "refresh", "cancel", "is_current", "recover_all_previously_opened_review_workspaces", "save", "restore_snapshot"];
  const methods = names.map((name, index): CodeNode => ({
    ...method, id: `dense-method-${index}`, name,
    signature: index === 5 ? `pub async fn ${name}(&mut self, options: RecoveryOptions, cancellation: CancellationToken) -> Result<Vec<ReviewWorkspace>, RecoveryError>`
      : `pub fn ${name}(&mut self) -> Result<()>`,
    code: `{ ${index === 0 ? "close();" : "Ok(())"} }`,
    references: index === 0 ? [{ semantic: "code.calls", target: { kind: "node", id: "dense-method-1" } }] : [],
  }));
  const contract: CodeNode = { ...type, id: "Persist", name: "Persist", nativeKind: "trait", signature: "pub trait Persist", contains: [] };
  for (const graph of [comparison.before, comparison.after]) {
    graph.nodes.push(contract);
    const storage = graph.nodes.find((node) => node.id === "storage")!;
    storage.contains = [...storage.contains, contract.id];
  }
  const traitImplementation: CodeNode = { ...implementation, id: "impl-persist", name: "impl Persist for Review", signature: "impl Persist for Review", contains: methods.slice(6).map((node) => node.id),
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
  const run: CodeNode = { ...template, id: "crate-run", name: "run", signature: "pub fn run()", contains: [], references: [] };
  const library: CodeNode = { ...run, id: "library-crate", category: "container", nativeKind: "crate", name: "src-tauri/src/lib.rs", signature: null, code: null, contains: [run.id] };
  const testRoot: CodeNode = { ...library, id: "test-crate", name: "src-tauri/tests/github_discovery.rs", contains: ["test-helper", "test-checks", "test-long"] };
  const helper: CodeNode = { ...run, id: "test-helper", name: "repository_with_remotes", signature: "fn repository_with_remotes(remotes: &[(&str, &str)]) -> TempDir" };
  const checks: CodeNode = { ...run, id: "test-checks", name: "discovers_pull_requests", signature: "#[test] fn discovers_pull_requests()",
    references: [{ semantic: "code.calls", target: { kind: "node", id: run.id } }, { semantic: "code.calls", target: { kind: "node", id: helper.id } }] };
  const long: CodeNode = { ...run, id: "test-long", name: "uses_authenticated_metadata_access_and_keeps_permission_failures_local",
    signature: "#[test] fn uses_authenticated_metadata_access_and_keeps_permission_failures_local()" };
  const added = [testRoot, helper, checks, long];
  return {
    ...comparison,
    before: { root: [library.id], nodes: [library, run], externalSymbols: {} },
    after: { root: [library.id, testRoot.id], nodes: [library, { ...run, code: "{ discover_pull_requests(); }" }, ...added], externalSymbols: {} },
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

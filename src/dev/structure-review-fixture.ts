import type { CodeNode, CodeReference, StructureComparison } from "@/structure-review/types";

/** 预览和应用测试共用的代表性图，包含高层对象、导入声明和独立的关系变化。 */
export function structureReviewFixture({ importsOnly = false }: { importsOnly?: boolean } = {}): StructureComparison {
  const call = (id: string): CodeReference => ({ semantic: "code.calls", target: { kind: "node", id } });
  const node = (id: string, category: CodeNode["category"], contains: string[] = [], extra: Partial<CodeNode> = {}): CodeNode => ({
    id, category, contains, language: "rust", nativeKind: category === "container" ? "mod" : category === "type" ? "struct" : category === "extension" ? "impl" : "fn",
    name: id, signature: category === "callable" ? `fn ${id}()` : `${category} ${id}`, code: null, references: [], extensions: {}, ...extra,
  });
  const shared = [
    node("narrant", "container", ["review", "storage"]),
    node("Review", "type"),
    node("impl", "extension", ["run"], { name: "impl Review", signature: "impl Review", references: [{ semantic: "code.member_of", target: { kind: "node", id: "Review" } }] }),
    node("keep", "callable", [], { name: "restore_reader_state" }),
    node("storage", "container", ["save", "load", "long", "use-unchanged"]),
    node("use-unchanged", "extension", [], { nativeKind: "use", name: null, signature: "use std::path::Path;" }),
    node("save", "callable", [], { name: "save_snapshot" }),
    node("load", "callable", [], { name: "load_snapshot" }),
    node("long", "callable", [], { name: "recover_all_previously_opened_review_workspaces_with_very_long_descriptive_names", signature: "pub async fn recover_all_previously_opened_review_workspaces_with_very_long_descriptive_names(&self) -> Result<(), RecoveryError>" }),
  ];
  const before: StructureComparison["before"] = { root: ["narrant"], externalSymbols: {}, nodes: [...shared,
    node("review", "container", ["Review", "impl", "old", "keep", "use-old"]),
    node("use-old", "extension", [], { nativeKind: "use", name: null, signature: "use crate::legacy::load;" }),
    node("old", "callable", [], { name: "load_legacy_state" }),
    node("run", "callable", [], { name: "run_review", code: "{ let state = load_legacy_state(); save_snapshot(state); }", references: [call("old"), call("save")] }),
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
    node("review", "container", ["Review", "impl", "new", "keep", "use-new"]),
    node("use-new", "extension", [], { nativeKind: "use", name: null, signature: "pub use crate::storage::load;", references: [unusedImport] }),
    node("new", "callable", [], { name: "prepare_snapshot", code: "{ save_snapshot(Snapshot::new()); }", references: addedReferences }),
    node("run", "callable", [], { name: "run_review", code: "{ let state = prepare_snapshot(); save_snapshot(state); generated::on_review_opened(); }", references: [call("new"), call("save"), unresolved] }),
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

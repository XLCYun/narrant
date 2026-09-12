import type { CodeNode, CodeReference, StructureComparison } from "@/structure-review/types";

/** 预览和应用测试共用的代表性图，包含 impl、跨容器调用和独立的关系变化。 */
export function structureReviewFixture(): StructureComparison {
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
    node("storage", "container", ["save", "load", "long"]),
    node("save", "callable", [], { name: "save_snapshot" }),
    node("load", "callable", [], { name: "load_snapshot" }),
    node("long", "callable", [], { name: "recover_all_previously_opened_review_workspaces_with_very_long_descriptive_names", signature: "pub async fn recover_all_previously_opened_review_workspaces_with_very_long_descriptive_names(&self) -> Result<(), RecoveryError>" }),
  ];
  const before = { root: ["narrant"], nodes: [...shared,
    node("review", "container", ["Review", "impl", "old", "keep"]),
    node("old", "callable", [], { name: "load_legacy_state" }),
    node("run", "callable", [], { name: "run_review", code: "{ let state = load_legacy_state(); save_snapshot(state); }", references: [call("old"), call("save")] }),
  ] };
  const unresolved: CodeReference = { semantic: "code.references", target: { kind: "unresolved", name: "generated::on_review_opened" } };
  const external: CodeReference = { semantic: "code.calls", target: { kind: "external", symbol: "rust:serde_json::to_string" } };
  const after = { root: ["narrant"], nodes: [...shared,
    node("review", "container", ["Review", "impl", "new", "keep"]),
    node("new", "callable", [], { name: "prepare_snapshot", code: "{ save_snapshot(Snapshot::new()); }", references: [call("save"), external] }),
    node("run", "callable", [], { name: "run_review", code: "{ let state = prepare_snapshot(); save_snapshot(state); generated::on_review_opened(); }", references: [call("new"), call("save"), unresolved] }),
  ] };
  return {
    reviewId: "structure-preview", snapshotId: "structure-snapshot", comparisonBase: "1111111", headRevision: "2222222", before, after,
    diff: { nodes: [
      { id: "review", content: null, contains: [{ kind: "removed", value: "old" }, { kind: "added", value: "new" }], references: [] },
      { id: "old", content: "removed", contains: [], references: [] },
      { id: "new", content: "added", contains: [], references: [{ kind: "added", value: call("save") }, { kind: "added", value: external }] },
      { id: "run", content: "modified", contains: [], references: [{ kind: "removed", value: call("old") }, { kind: "added", value: call("new") }, { kind: "added", value: unresolved }] },
    ] },
    coverage: [
      { path: "src/review.rs", oldPath: null, before: "analyzed", after: "analyzed" },
      { path: "src/storage.rs", oldPath: null, before: "analyzed", after: "analyzed" },
      { path: "src/structure-changes-view.tsx", oldPath: null, before: "unsupported-language", after: "unsupported-language" },
    ],
  };
}

import { beforeAll, describe, expect, it } from "vitest";
import {
  collapseStructureLayout,
  layoutStructureComparison,
  relayoutStructureLayout,
  selectStructureEdges,
  type GraphOptions,
  type StructureEdge,
  type StructureLayout,
} from "@/structure-review/graph-presentation";
import type { CodeNode, CodeReference, StructureComparison } from "@/structure-review/types";

const options: GraphOptions = { showAll: true, showUnchangedReferences: true, showImports: false, expanded: new Set() };

/** 关系只使用真实对象身份，不借用文件路径或卡片 ID。 */
function reference(id: string, semantic = "code.calls"): CodeReference {
  return { semantic, target: { kind: "node", id } };
}

/** 用最小声明构造展示层输入，保留可区分的对象类别与源码容器。 */
function node(id: string, category: CodeNode["category"], extra: Partial<CodeNode> = {}): CodeNode {
  return {
    id, category, language: "rust", nativeKind: category === "container" ? "mod" : category === "type" ? "struct" : category === "extension" ? "impl" : "fn",
    name: id, signature: null, code: null, source: null, contains: [], references: [], extensions: {}, ...extra,
  };
}

/** 跨文件实现块归入类型，同时提供嵌套类型、入向调用、删除关系和无关调用。 */
function selectionFixture(): StructureComparison {
  const nodes = [
    node("feature-file", "container", { nativeKind: "file", name: "src/features/review.rs", contains: ["review"] }),
    node("review", "container", { contains: ["Review", "nested"] }),
    node("Review", "type", { contains: ["field", "sibling", "NestedType"] }),
    node("field", "value", { nativeKind: "field", references: [reference("Store", "code.type_use")] }),
    node("sibling", "callable", { references: [reference("cleanup")] }),
    node("NestedType", "type", { contains: ["deep"] }),
    node("deep", "callable", { references: [reference("save")] }),
    node("nested", "container", { contains: ["helper"] }),
    node("helper", "callable", { references: [reference("save"), reference("Store", "code.imports")] }),
    node("implementation-file", "container", { nativeKind: "file", name: "src/implementations/review.rs", contains: ["impl"] }),
    node("impl", "extension", { contains: ["run", "restore"], references: [reference("Review", "code.applies_to")] }),
    node("run", "callable", { references: [reference("save"), reference("sibling")] }),
    node("restore", "callable", { references: [reference("cleanup")] }),
    node("storage-file", "container", { nativeKind: "file", name: "src/storage/store.rs", contains: ["Store"] }),
    node("Store", "type", { contains: ["save", "cleanup", "legacy"] }),
    node("save", "callable"),
    node("cleanup", "callable"),
    node("legacy", "callable"),
    node("caller-file", "container", { nativeKind: "file", name: "tests/entry.rs", contains: ["caller", "bystander", "src/features"] }),
    node("caller", "callable", { references: [reference("run")] }),
    node("bystander", "callable", { references: [reference("cleanup")] }),
    node("src/features", "callable", { references: [reference("cleanup")] }),
  ];
  const graph = { root: ["feature-file", "implementation-file", "storage-file", "caller-file"], externalSymbols: {}, nodes };
  const before = structuredClone(graph);
  before.nodes.find((object) => object.id === "run")!.references.push(reference("legacy"));
  return {
    reviewId: "selection-review", snapshotId: "selection-snapshot", comparisonBase: "before", headRevision: "after",
    before, after: graph, coverage: [],
    diff: { nodes: [{ id: "run", content: "modified", contains: [], references: [{ kind: "removed", value: reference("legacy") }] }] },
  };
}

/** 以原始关系身份比较结果，避免依赖 ELK 顺序与坐标。 */
function relations(edges: StructureEdge[]): string[] {
  return edges.map((edge) => `${edge.data!.sourceObjectId} -> ${edge.data!.targetObjectId}: ${edge.label}`).sort();
}

/** 找到对象当前所在的标题或成员表卡片。 */
function card(layout: StructureLayout, id: string) {
  return layout.nodes.find((item) => item.data.objectId === id || item.data.rows.some((row) => row.objectId === id))!;
}

/** 找到当前对象的独立端口，折叠容器使用其标题端口。 */
function port(layout: StructureLayout, id: string, side: "source" | "target"): string {
  return card(layout, id).data.ports.find((item) => item.objectId === id && item.side === side)!.id;
}

/** 提供类型所有后代的双向关系，包含嵌套声明和跨文件实现的方法。 */
function typeRelations(): string[] {
  return [
    "caller -> run: code.calls",
    "deep -> save: code.calls",
    "field -> Store: code.type_use",
    "restore -> cleanup: code.calls",
    "run -> legacy: − code.calls",
    "run -> save: code.calls",
    "run -> sibling: code.calls",
    "sibling -> cleanup: code.calls",
  ].sort();
}

describe("structure selection relations", () => {
  let comparison: StructureComparison;
  let expanded: StructureLayout;

  beforeAll(async () => {
    comparison = selectionFixture();
    expanded = await layoutStructureComparison(comparison, options);
  });

  it("shows no relations before selection or for an absent object or directory", () => {
    expect(selectStructureEdges(expanded, expanded)).toEqual([]);
    expect(selectStructureEdges(expanded, expanded, { kind: "object", id: "absent" })).toEqual([]);
    expect(selectStructureEdges(expanded, expanded, { kind: "directory", path: "absent" })).toEqual([]);
  });

  it("inherits incoming and outgoing relations through displayed type membership and nested cards", () => {
    const selected = selectStructureEdges(expanded, expanded, { kind: "object", id: "Review" });
    expect(relations(selected)).toEqual(typeRelations());
    expect(card(expanded, "run").data.rows.some((row) => row.objectId === "impl")).toBe(true);
    expect(card(expanded, "deep").parentId).toBe(card(expanded, "Review").id);
    for (const edge of selected) {
      expect(edge.sourceHandle).toBe(port(expanded, edge.data!.sourceObjectId, "source"));
      expect(edge.targetHandle).toBe(port(expanded, edge.data!.targetObjectId, "target"));
    }
    expect(selectStructureEdges(expanded, expanded, { kind: "object", id: "implementation-file" })).toEqual([]);
  });

  it("inherits all descendant relations for modules and files while excluding unrelated callers", () => {
    const expected = [...typeRelations(), "helper -> save: code.calls"].sort();
    for (const id of ["review", "feature-file"]) {
      expect(relations(selectStructureEdges(expanded, expanded, { kind: "object", id }))).toEqual(expected);
    }
    expect(relations(selectStructureEdges(expanded, expanded, { kind: "object", id: "nested" }))).toEqual(["helper -> save: code.calls"]);
  });

  it("selects only a leaf member's own incoming and outgoing relations despite sharing a card", () => {
    expect(card(expanded, "run").id).toBe(card(expanded, "restore").id);
    expect(card(expanded, "run").id).toBe(card(expanded, "sibling").id);
    expect(relations(selectStructureEdges(expanded, expanded, { kind: "object", id: "run" }))).toEqual([
      "caller -> run: code.calls", "run -> legacy: − code.calls", "run -> save: code.calls", "run -> sibling: code.calls",
    ]);
    expect(relations(selectStructureEdges(expanded, expanded, { kind: "object", id: "restore" }))).toEqual(["restore -> cleanup: code.calls"]);
  });

  it("retains descendant relations after a type or ancestor module is collapsed without reviving internal edges", () => {
    for (const id of ["Review", "review", "feature-file"]) {
      const collapsed = collapseStructureLayout(expanded, new Set([id]));
      const expected = typeRelations().filter((value) => value !== "run -> sibling: code.calls");
      if (id !== "Review") expected.push("helper -> save: code.calls");
      const selected = selectStructureEdges(expanded, collapsed, { kind: "object", id });
      expect(relations(selected)).toEqual(expected.sort());
      const outgoing = selected.find((edge) => edge.data!.sourceObjectId === "run" && edge.data!.targetObjectId === "save")!;
      const incoming = selected.find((edge) => edge.data!.sourceObjectId === "caller")!;
      expect(outgoing.sourceHandle).toBe(port(collapsed, id, "source"));
      expect(incoming.targetHandle).toBe(port(collapsed, id, "target"));
      expect(card(collapsed, "run")).toBeUndefined();
    }
  });

  it("connects both collapsed containers while retaining each original member relation and removed status", () => {
    const collapsed = collapseStructureLayout(expanded, new Set(["Review", "Store"]));
    const selected = selectStructureEdges(expanded, collapsed, { kind: "object", id: "Review" });
    const crossContainer = selected.filter((edge) => edge.data!.sourceObjectId !== "caller");
    expect(crossContainer).toHaveLength(6);
    for (const edge of crossContainer) {
      expect(edge.source).toBe(card(collapsed, "Review").id);
      expect(edge.target).toBe(card(collapsed, "Store").id);
      expect(edge.sourceHandle).toBe(port(collapsed, "Review", "source"));
      expect(edge.targetHandle).toBe(port(collapsed, "Store", "target"));
      const original = expanded.edges.find((candidate) => candidate.id === edge.id)!;
      expect(edge.data).toMatchObject({
        sourceObjectId: original.data!.sourceObjectId, targetObjectId: original.data!.targetObjectId,
        sourceName: original.data!.sourceName, targetName: original.data!.targetName,
      });
      expect(edge.label).toBe(original.label);
      expect(edge.style).toEqual(original.style);
    }
    const removed = crossContainer.find((edge) => edge.data!.targetObjectId === "legacy")!;
    expect(removed.label).toBe("− code.calls");
    expect(removed.data).toMatchObject({ collapsedSource: "Review::run", collapsedTarget: "Store::legacy" });
    expect(selected.every((edge) => selectStructureEdges(expanded, collapsed, { kind: "object", id: "Store" }).includes(edge) || edge.data!.sourceObjectId === "caller")).toBe(true);
  });

  it("keeps directory paths separate from opaque object IDs and follows displayed file descendants", () => {
    const expected = [...typeRelations(), "helper -> save: code.calls"].sort();
    expect(relations(selectStructureEdges(expanded, expanded, { kind: "directory", path: "src/features" }))).toEqual(expected);
    expect(relations(selectStructureEdges(expanded, expanded, { kind: "object", id: "src/features" }))).toEqual(["src/features -> cleanup: code.calls"]);
    expect(selectStructureEdges(expanded, expanded, { kind: "directory", path: "src/implementations" })).toEqual([]);
    const collapsed = collapseStructureLayout(expanded, new Set(), new Set(["src/features", "src/storage"]));
    const selected = selectStructureEdges(expanded, collapsed, { kind: "directory", path: "src/features" });
    expect(relations(selected)).toEqual(expected.filter((value) => value !== "run -> sibling: code.calls"));
    const sourceDirectory = collapsed.nodes.find((item) => item.data.directory?.path === "src/features")!;
    const targetDirectory = collapsed.nodes.find((item) => item.data.directory?.path === "src/storage")!;
    const crossContainer = selected.filter((edge) => edge.data!.sourceObjectId !== "caller");
    expect(crossContainer).toHaveLength(7);
    for (const edge of crossContainer) {
      expect(edge.source).toBe(sourceDirectory.id);
      expect(edge.target).toBe(targetDirectory.id);
      expect(edge.sourceHandle).toBe(sourceDirectory.data.ports.find((item) => item.side === "source")!.id);
      expect(edge.targetHandle).toBe(targetDirectory.data.ports.find((item) => item.side === "target")!.id);
    }
  });

  it("respects relationship visibility options and preserves selection through optional relayout", async () => {
    for (const showUnchangedReferences of [false, true]) {
      const base = await layoutStructureComparison(comparison, { ...options, showUnchangedReferences, showImports: true });
      const collapsed = collapseStructureLayout(base, new Set(["review", "Store"]));
      const expected = showUnchangedReferences
        ? [...typeRelations().filter((value) => value !== "run -> sibling: code.calls"), "helper -> Store: code.imports", "helper -> save: code.calls"].sort()
        : ["field -> Store: code.type_use", "run -> legacy: − code.calls"];
      expect(relations(selectStructureEdges(base, collapsed, { kind: "object", id: "review" }))).toEqual(expected);
      const rearranged = await relayoutStructureLayout(collapsed);
      expect(relations(selectStructureEdges(base, rearranged, { kind: "object", id: "review" }))).toEqual(expected);
      for (const edge of selectStructureEdges(base, rearranged, { kind: "object", id: "review" })) {
        expect(rearranged.edges).toContain(edge);
      }
    }
  });

  it("does not change the comparison, expanded baseline, collapsed ports or routed edges", () => {
    const collapsed = collapseStructureLayout(expanded, new Set(["Review", "Store"]));
    const originalComparison = structuredClone(comparison);
    const originalExpanded = structuredClone(expanded);
    const originalCollapsed = structuredClone(collapsed);
    for (const layout of [expanded, collapsed]) {
      selectStructureEdges(expanded, layout, { kind: "object", id: "Review" });
      selectStructureEdges(expanded, layout, { kind: "directory", path: "src/features" });
      selectStructureEdges(expanded, layout, { kind: "object", id: "run" });
    }
    expect(comparison).toEqual(originalComparison);
    expect(expanded).toEqual(originalExpanded);
    expect(collapsed).toEqual(originalCollapsed);
  });
});

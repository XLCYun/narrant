import { describe, expect, it } from "vitest";
import { structureCrateRootsFixture, structureFieldTypesFixture, structureReviewFixture } from "@/dev/structure-review-fixture";
import { layoutStructureComparison, objectDetails, type StructureLayout } from "@/structure-review/graph-presentation";
import type { CodeNode, CodeReference, NodeDifference, StructureComparison } from "@/structure-review/types";

const options = { showAll: false, showUnchangedReferences: false, showImports: false, expanded: new Set<string>() };

/** 从公开布局找到一个对象，兼容卡片标题和卡片内的成员行。 */
function entry(layout: StructureLayout, id: string) {
  const card = layout.nodes.find((node) => node.data.objectId === id || node.data.rows.some((row) => row.objectId === id));
  if (!card) return undefined;
  const row = card.data.rows.find((row) => row.objectId === id);
  return { card, object: row ?? card.data, row };
}

/** 对象身份决定端口，即使源方法与目标方法共用一张卡片也不能混淆。 */
function endpoint(layout: StructureLayout, id: string, side: "source" | "target") {
  const { card } = entry(layout, id)!;
  return { card: card.id, handle: card.data.ports.find((port) => port.objectId === id && port.side === side)!.id };
}

describe("Structure Graph presentation Interface", () => {
  it("prepares unchanged field types as context with exact member-to-type ports", async () => {
    const comparison = structureFieldTypesFixture();
    const original = structuredClone(comparison);
    const layout = await layoutStructureComparison(comparison, options);
    const source = endpoint(layout, "pull-requests", "source"), target = endpoint(layout, "Item", "target");
    expect(entry(layout, "Item")?.object.status).toBe("unchanged");
    expect(entry(layout, "Item")?.object.hiddenChildren).toBe(1);
    expect(entry(layout, "item-title")).toBeUndefined();
    expect(entry(layout, "store")).toBeUndefined();
    expect(layout.edges.some((edge) => edge.sourceHandle === source.handle && edge.targetHandle === target.handle && edge.label === "code.type_use")).toBe(true);
    expect(layout.edges.some((edge) => String(edge.label).includes("code.calls"))).toBe(false);
    const replacement = endpoint(layout, "repository", "source");
    for (const [id, label] of [["Repository", "+ code.type_use"], ["LegacyRepository", "− code.type_use"]]) {
      expect(layout.edges.some((edge) => edge.sourceHandle === replacement.handle && edge.targetHandle === endpoint(layout, id, "target").handle && edge.label === label)).toBe(true);
    }
    const recursive = endpoint(layout, "parent", "source"), group = endpoint(layout, "Group", "target");
    expect(layout.edges.some((edge) => edge.source === group.card && edge.target === group.card && edge.sourceHandle === recursive.handle && edge.targetHandle === group.handle)).toBe(true);
    const allRelations = await layoutStructureComparison(comparison, { ...options, showUnchangedReferences: true });
    expect(entry(allRelations, "store")).toBeDefined();
    expect(allRelations.edges.some((edge) => edge.label === "code.calls")).toBe(true);
    expect(comparison).toEqual(original);
  });

  it("keeps graph-external and unresolved field types out of the layout and details", async () => {
    const comparison = structureFieldTypesFixture();
    for (const side of [comparison.before, comparison.after]) {
      side.nodes.find((node) => node.id === "pull-requests")!.references.push(
        { semantic: "code.type_use", target: { kind: "external", symbol: "rust:sysroot:Vec" } },
        { semantic: "code.type_use", target: { kind: "external", symbol: "rust:repo:Unselected" } },
        { semantic: "code.type_use", target: { kind: "unresolved", name: "Missing" } },
      );
    }
    const layout = await layoutStructureComparison(comparison, { ...options, showAll: true, showUnchangedReferences: true });
    const source = endpoint(layout, "pull-requests", "source");
    const outgoing = layout.edges.filter((edge) => edge.sourceHandle === source.handle);
    expect(outgoing).toHaveLength(1);
    expect(outgoing[0].targetHandle).toBe(endpoint(layout, "Item", "target").handle);
    expect(objectDetails(comparison, "pull-requests").after?.references).toEqual([{ semantic: "code.type_use", target: { kind: "node", id: "Item" } }]);
  });

  it("keeps library and integration-test root members in their own crate cards", async () => {
    const comparison = structureCrateRootsFixture();
    const layout = await layoutStructureComparison(comparison, options);
    const library = entry(layout, "library-crate")!, tests = entry(layout, "test-crate")!;
    expect(library.object.status).toBe("unchanged");
    expect(tests.object.status).toBe("added");
    expect(library.object.label).toBe("src-tauri/src/lib.rs");
    expect(tests.object.label).toBe("src-tauri/tests/github_discovery.rs");
    expect(entry(layout, "crate-run")?.card.id).toBe(library.card.id);
    for (const id of ["test-helper", "test-checks", "test-long"]) expect(entry(layout, id)?.card.id).toBe(tests.card.id);
    expect(library.card.id).not.toBe(tests.card.id);
    expect(layout.nodes.some((node) => node.id === "members-root")).toBe(false);
    const source = endpoint(layout, "test-checks", "source"), target = endpoint(layout, "crate-run", "target");
    expect(layout.edges.some((edge) => edge.source === tests.card.id && edge.target === library.card.id && edge.sourceHandle === source.handle && edge.targetHandle === target.handle)).toBe(true);
    expect(objectDetails(comparison, "test-checks").after?.name).toBe("discovers_pull_requests");
  });
  it("keeps own content changes separate from relationship changes and includes containing context", async () => {
    const layout = await layoutStructureComparison(structureReviewFixture(), options);
    const object = (id: string) => entry(layout, id);
    expect(object("run")?.object.status).toBe("modified");
    expect(object("snapshot")?.object.status).toBe("modified");
    expect(object("Review")?.object.status).toBe("unchanged");
    expect(object("review")?.object.status).toBe("relations");
    expect(object("impl")?.object.status).toBe("unchanged");
    expect(object("new")?.object.status).toBe("added");
    expect(object("old")?.object.status).toBe("removed");
    expect(object("run")?.card.id).toBe(object("Review")?.card.id);
    expect(object("snapshot")?.card.id).toBe(object("Review")?.card.id);
    expect(object("impl")?.row?.kind).toBe("implementation");
    expect(object("keep")).toBeUndefined();
    expect(object("storage")?.object.hiddenChildren).toBe(2);
    const source = endpoint(layout, "new", "source"), target = endpoint(layout, "save", "target");
    expect(layout.edges.some((edge) => edge.sourceHandle === source.handle && edge.targetHandle === target.handle)).toBe(true);
    for (const child of layout.nodes.filter((node) => node.parentId)) {
      const parent = layout.nodes.find((node) => node.id === child.parentId)!;
      expect(layout.nodes.indexOf(parent)).toBeLessThan(layout.nodes.indexOf(child));
      expect(child.position.x).toBeGreaterThanOrEqual(0);
      expect(child.position.y).toBeGreaterThanOrEqual(parent.data.headerHeight);
      expect(child.position.x + Number(child.style?.width)).toBeLessThanOrEqual(Number(parent.style?.width));
      expect(child.position.y + Number(child.style?.height)).toBeLessThanOrEqual(Number(parent.style?.height));
    }
  });

  it("expands unchanged children without changing analysis scope", async () => {
    const comparison = structureReviewFixture();
    const layout = await layoutStructureComparison(comparison, { ...options, expanded: new Set(["storage"]) });
    expect(entry(layout, "long")?.object.label).toContain("very_long");
    expect(entry(layout, "keep")).toBeUndefined();
    const all = await layoutStructureComparison(comparison, { ...options, showAll: true, showUnchangedReferences: true });
    expect(all.visible).toBe(all.total);
    expect(all.edges.some((edge) => edge.source === edge.target && ["code.member_of", "code.applies_to"].includes(String(edge.label)))).toBe(false);
    expect(objectDetails(comparison, "run").after?.references.some((reference) => reference.semantic === "code.member_of")).toBe(true);
    expect(objectDetails(comparison, "impl").after?.references.some((reference) => reference.semantic === "code.applies_to")).toBe(true);
  });

  it("does not union opposite before/after containment into a cycle", async () => {
    const comparison = structureReviewFixture();
    const a = { ...comparison.before.nodes[0], id: "a", contains: ["b"] };
    const b = { ...a, id: "b", contains: [] };
    comparison.before = { root: ["a"], nodes: [a, b], externalSymbols: {} };
    comparison.after = { root: ["b"], nodes: [{ ...a, contains: [] }, { ...b, contains: ["a"] }], externalSymbols: {} };
    comparison.diff = { nodes: [
      { id: "a", content: null, contains: [{ kind: "removed", value: "b" }], references: [] },
      { id: "b", content: null, contains: [{ kind: "added", value: "a" }], references: [] },
    ] };
    const layout = await layoutStructureComparison(comparison, options);
    const parent = layout.nodes.find((node) => node.data.objectId === "b")!;
    expect(parent.parentId).toBeUndefined();
    expect(layout.nodes.find((node) => node.data.objectId === "a")?.parentId).toBe(parent.id);
    expect(layout.edges[0].label).toBe("− contains");
  });
});

it("omits all external and unresolved targets in every display mode without changing the analysis", async () => {
  const comparison = structureReviewFixture();
  const original = structuredClone(comparison);
  const objects = new Set([...comparison.before.nodes, ...comparison.after.nodes].map((node) => node.id));
  for (const showAll of [false, true]) for (const showUnchangedReferences of [false, true]) for (const showImports of [false, true]) {
    const layout = await layoutStructureComparison(comparison, { ...options, showAll, showUnchangedReferences, showImports, expanded: new Set(["review", "storage"]) });
    for (const node of layout.nodes) {
      const ids = [node.data.objectId, ...node.data.rows.map((row) => row.objectId)].filter((id) => id !== undefined);
      expect(ids.length).toBeGreaterThan(0);
      expect(ids.every((id) => objects.has(id))).toBe(true);
    }
    const ids = new Set(layout.nodes.map((node) => node.id));
    expect(layout.edges.every((edge) => ids.has(edge.source) && ids.has(edge.target))).toBe(true);
    const source = endpoint(layout, "new", "source"), target = endpoint(layout, "save", "target");
    expect(layout.edges.some((edge) => edge.sourceHandle === source.handle && edge.targetHandle === target.handle && edge.label === "+ code.calls")).toBe(true);
  }
  for (const id of objects) {
    const details = objectDetails(comparison, id);
    for (const side of [details.before, details.after]) expect(side?.references.every((reference) => reference.target.kind === "node") ?? true).toBe(true);
    expect(details.difference?.references.every((change) => change.value.target.kind === "node") ?? true).toBe(true);
  }
  expect(comparison).toEqual(original);
});

it("only includes import-only targets when imports are enabled", async () => {
  const comparison = structureReviewFixture();
  const hidden = await layoutStructureComparison(comparison, options);
  expect(entry(hidden, "load")).toBeUndefined();
  expect(hidden.edges.some((edge) => String(edge.label).includes("code.imports"))).toBe(false);
  const shown = await layoutStructureComparison(comparison, { ...options, showImports: true });
  expect(entry(shown, "load")).toBeDefined();
  expect(shown.edges.some((edge) => edge.label === "+ code.imports")).toBe(true);
});

it("omits Rust use declarations and incident relations from every display mode without changing the analysis", async () => {
  const comparison = structureReviewFixture();
  const useReference = { semantic: "code.references", target: { kind: "node" as const, id: "use-new" } };
  comparison.after.nodes.find((node) => node.id === "new")!.references.push(useReference);
  comparison.diff.nodes.find((node) => node.id === "new")!.references.push({ kind: "added", value: useReference });
  const original = structuredClone(comparison);
  for (const overrides of [{}, { expanded: new Set(["review", "storage"]) }, { showAll: true, showImports: true, showUnchangedReferences: true }]) {
    const layout = await layoutStructureComparison(comparison, { ...options, ...overrides });
    expect(layout.nodes.some((node) => node.data.nativeKind === "use" || node.data.label === "imported_but_unused")).toBe(false);
    expect(layout.changed).toBe(5);
    expect(layout.total).toBe(15);
    const source = endpoint(layout, "new", "source"), target = endpoint(layout, "save", "target");
    expect(layout.edges.some((edge) => edge.sourceHandle === source.handle && edge.targetHandle === target.handle && edge.label === "+ code.calls")).toBe(true);
    const ids = new Set(layout.nodes.map((node) => node.id));
    expect(layout.edges.every((edge) => ids.has(edge.source) && ids.has(edge.target))).toBe(true);
  }
  const details = objectDetails(comparison, "review");
  expect(details.difference?.contains.map((change) => change.value)).toEqual(["old", "new"]);
  expect(details.after?.contains).not.toContain("use-new");
  expect(objectDetails(comparison, "new").after?.references).not.toContainEqual(useReference);
  expect(objectDetails(comparison, "use-new").after).toBeUndefined();
  expect(comparison).toEqual(original);
});

it("keeps import-only changes out of parent status, counts and expansion hints", async () => {
  const comparison = structureReviewFixture({ importsOnly: true });
  const hidden = await layoutStructureComparison(comparison, options);
  expect(hidden).toMatchObject({ nodes: [], edges: [], changed: 0, total: 1, visible: 0 });
  const all = await layoutStructureComparison(comparison, { ...options, showAll: true, showImports: true, expanded: new Set(["review"]) });
  expect(all.nodes).toHaveLength(1);
  expect(all.nodes[0].data).toMatchObject({ objectId: "review", status: "unchanged", hiddenChildren: 0 });
  expect(all.edges).toEqual([]);
  expect(objectDetails(comparison, "review").difference).toBeUndefined();
  expect(comparison.diff.nodes).toHaveLength(3);
});

it.each(["repository", "dependency", undefined] as const)("hides external references with %s scope, including missing descriptors", async (scope) => {
  const comparison = structureReviewFixture();
  const reference = { semantic: "code.calls", target: { kind: "external" as const, symbol: "opaque:%2F" } };
  comparison.after.nodes.find((node) => node.id === "new")!.references = [reference];
  comparison.diff.nodes.find((node) => node.id === "new")!.references = [{ kind: "added", value: reference }];
  if (scope) comparison.after.externalSymbols[reference.target.symbol] = { name: "工具::处理", scope };
  const hidden = await layoutStructureComparison(comparison, { ...options, showAll: true, showUnchangedReferences: true, showImports: true });
  expect(hidden.nodes.some((node) => node.data.label === "工具::处理")).toBe(false);
  expect(hidden.nodes.some((node) => node.data.label === reference.target.symbol)).toBe(false);
  expect(objectDetails(comparison, "new").after?.references).toEqual([]);
  expect(objectDetails(comparison, "new").difference).toMatchObject({ content: "added", references: [] });
});

it("omits removed external references while preserving removed local references", async () => {
  const comparison = structureReviewFixture();
  const reference = { semantic: "code.calls", target: { kind: "external" as const, symbol: "old-opaque-symbol" } };
  const local = relation("code.calls", "save");
  comparison.before.nodes.find((node) => node.id === "old")!.references = [reference, local];
  comparison.diff.nodes.find((node) => node.id === "old")!.references = [reference, local].map((value) => ({ kind: "removed", value }));
  comparison.before.externalSymbols[reference.target.symbol] = { name: "旧模块::旧函数", scope: "repository" };
  const layout = await layoutStructureComparison(comparison, options);
  expect(layout.nodes.some((node) => node.data.label === "旧模块::旧函数")).toBe(false);
  const source = endpoint(layout, "old", "source"), target = endpoint(layout, "save", "target");
  expect(layout.edges.some((edge) => edge.sourceHandle === source.handle && edge.targetHandle === target.handle && edge.label === "− code.calls")).toBe(true);
  const details = objectDetails(comparison, "old");
  expect(details.before?.references).toEqual([local]);
  expect(details.difference?.references).toEqual([{ kind: "removed", value: local }]);
});

it("does not mark objects changed solely for hidden external or unresolved relationships", async () => {
  const references: CodeReference[] = [
    { semantic: "code.calls", target: { kind: "external", symbol: "external-function" } },
    { semantic: "code.calls", target: { kind: "unresolved", name: "unknown_function" } },
  ];
  const parent = diagramNode("module", "container", { contains: ["source"] });
  const source = diagramNode("source", "callable");
  const comparison = diagramComparison([parent, source], [parent, { ...source, references }], [
    { id: source.id, content: null, contains: [], references: references.map((value) => ({ kind: "added", value })) },
  ]);
  const hidden = await layoutStructureComparison(comparison, options);
  expect(hidden).toMatchObject({ nodes: [], edges: [], changed: 0, visible: 0, total: 2 });
  const all = await layoutStructureComparison(comparison, { ...options, showAll: true, showUnchangedReferences: true });
  expect(entry(all, source.id)?.object.status).toBe("unchanged");
  expect(entry(all, parent.id)?.object.status).toBe("unchanged");
  expect(all.edges).toEqual([]);
  expect(objectDetails(comparison, source.id).difference).toBeUndefined();
  expect(comparison.after.nodes.find((node) => node.id === source.id)?.references).toEqual(references);
});

it("stops abandoned layout requests through the public Interface", async () => {
  const abort = new AbortController();
  const running = layoutStructureComparison(structureReviewFixture(), options, abort.signal);
  abort.abort();
  await expect(running).rejects.toMatchObject({ name: "AbortError" });
});

/** 用最小的有效图组合验证展示语义，避免依赖某种语言的 ID 编码。 */
function diagramNode(id: string, category: CodeNode["category"], extra: Partial<CodeNode> = {}): CodeNode {
  return { id, category, language: "rust", nativeKind: category, name: id, signature: id, code: null, contains: [], references: [], extensions: {}, ...extra };
}

/** 只由声明的 contains 计算根，成员关系不会混进真实包含树。 */
function diagramComparison(before: CodeNode[], after: CodeNode[], differences: NodeDifference[] = []): StructureComparison {
  const graph = (nodes: CodeNode[]) => {
    const contained = new Set(nodes.flatMap((node) => node.contains));
    return { root: nodes.filter((node) => !contained.has(node.id)).map((node) => node.id), nodes, externalSymbols: {} };
  };
  return { ...structureReviewFixture(), before: graph(before), after: graph(after), diff: { nodes: differences } };
}

/** 构造明确的本地关联，测试只通过公开展示入口观察结果。 */
function relation(semantic: string, id: string): CodeReference {
  return { semantic, target: { kind: "node", id } };
}

it("renders class attributes and methods as compact rows with one occurrence per code object", async () => {
  const fields = Array.from({ length: 4 }, (_, index) => diagramNode(`field-${index}`, "value", { nativeKind: "property", signature: `name${index}: string`, language: "typescript" }));
  const methods = Array.from({ length: 8 }, (_, index) => diagramNode(`method-${index}`, "callable", { nativeKind: "method", signature: `handle${index}(request: Request): Promise<Response>`, language: "typescript" }));
  const nodes = [diagramNode("Client", "type", { nativeKind: "class", language: "typescript", contains: [...fields, ...methods].map((node) => node.id) }), ...fields, ...methods];
  const comparison = diagramComparison(nodes, nodes);
  const original = structuredClone(comparison);
  const layout = await layoutStructureComparison(comparison, { ...options, showAll: true });
  expect(layout.nodes).toHaveLength(1);
  const card = layout.nodes[0];
  expect(card.data.objectId).toBe("Client");
  expect(card.data.rows.filter((row) => row.kind === "value")).toHaveLength(4);
  expect(card.data.rows.filter((row) => row.kind === "callable")).toHaveLength(8);
  expect(card.data.rows.filter((row) => row.kind === "section").map((row) => row.section)).toEqual(["values", "callables"]);
  expect(card.data.rows.every((row) => row.height <= 36)).toBe(true);
  expect(Number(card.style!.height)).toBeLessThan(500);
  expect(new Set([card.data.objectId, ...card.data.rows.flatMap((row) => row.objectId ? [row.objectId] : [])]).size).toBe(layout.visible);
  for (const row of card.data.rows.filter((row) => row.objectId)) {
    expect(card.data.ports.filter((port) => port.objectId === row.objectId)).toHaveLength(2);
    expect(card.data.ports.filter((port) => port.objectId === row.objectId).every((port) => port.y === row.top + row.height / 2)).toBe(true);
  }
  expect(comparison).toEqual(original);
});

it("keeps internal calls and recursive calls attached to distinct member ports", async () => {
  const nodes = [diagramNode("Widget", "type", { contains: ["first", "second"] }), diagramNode("first", "callable"), diagramNode("second", "callable")];
  const references = [relation("code.calls", "first"), relation("code.calls", "second"), relation("code.member_of", "Widget")];
  const after = nodes.map((node) => node.id === "first" ? { ...node, references } : node);
  const comparison = diagramComparison(nodes, after, [{ id: "first", content: "modified", contains: [], references: references.map((value) => ({ kind: "added", value })) }]);
  const layout = await layoutStructureComparison(comparison, options);
  expect(layout.nodes).toHaveLength(1);
  expect(layout.edges).toHaveLength(2);
  const source = endpoint(layout, "first", "source");
  expect(layout.edges.every((edge) => edge.source === source.card && edge.target === source.card && edge.sourceHandle === source.handle)).toBe(true);
  expect(new Set(layout.edges.map((edge) => edge.targetHandle))).toEqual(new Set([endpoint(layout, "first", "target").handle, endpoint(layout, "second", "target").handle]));
  expect(layout.edges.every((edge) => edge.data?.route?.path.startsWith("M") && !edge.data.route.path.includes("NaN"))).toBe(true);
});

it("keeps moved members once in the after class and routes removed containment to the member", async () => {
  const a = diagramNode("A", "type", { contains: ["method"] }), b = diagramNode("B", "type"), method = diagramNode("method", "callable");
  const comparison = diagramComparison([a, b, method], [{ ...a, contains: [] }, { ...b, contains: ["method"] }, method], [
    { id: "A", content: null, contains: [{ kind: "removed", value: "method" }], references: [] },
    { id: "B", content: null, contains: [{ kind: "added", value: "method" }], references: [] },
  ]);
  const layout = await layoutStructureComparison(comparison, options);
  expect(entry(layout, "method")!.card.id).toBe(entry(layout, "B")!.card.id);
  expect(entry(layout, "method")!.object.status).toBe("unchanged");
  expect(layout.nodes.flatMap((node) => node.data.rows).filter((row) => row.objectId === "method")).toHaveLength(1);
  expect(layout.edges).toEqual([expect.objectContaining({ source: entry(layout, "A")!.card.id, targetHandle: endpoint(layout, "method", "target").handle, label: "− contains" })]);
});

it("retains implementation changes and trait links while grouping methods under the selected type", async () => {
  const a = diagramNode("A", "type"), b = diagramNode("B", "type"), trait = diagramNode("Contract", "type", { nativeKind: "trait" });
  const implementation = diagramNode("impl", "extension", { nativeKind: "impl", signature: "impl Contract for A", contains: ["method"], references: [relation("code.applies_to", "A"), relation("code.implements", "Contract")] });
  const method = diagramNode("method", "callable");
  const afterImpl = { ...implementation, signature: "impl Contract for B", references: [relation("code.applies_to", "B"), relation("code.implements", "Contract")] };
  const comparison = diagramComparison([a, b, trait, implementation, method], [a, b, trait, afterImpl, method], [{
    id: "impl", content: "modified", contains: [], references: [{ kind: "removed", value: relation("code.applies_to", "A") }, { kind: "added", value: relation("code.applies_to", "B") }],
  }]);
  const layout = await layoutStructureComparison(comparison, { ...options, showAll: true, showUnchangedReferences: true });
  expect(entry(layout, "impl")!.card.id).toBe(entry(layout, "B")!.card.id);
  expect(entry(layout, "method")!.card.id).toBe(entry(layout, "B")!.card.id);
  expect(entry(layout, "impl")!.object).toMatchObject({ label: "impl Contract for B", status: "modified" });
  expect(entry(layout, "B")!.object.status).toBe("unchanged");
  expect(layout.edges.some((edge) => edge.label === "− code.applies_to" && edge.targetHandle === endpoint(layout, "A", "target").handle)).toBe(true);
  expect(layout.edges.some((edge) => edge.label === "code.implements" && edge.sourceHandle === endpoint(layout, "impl", "source").handle && edge.targetHandle === endpoint(layout, "Contract", "target").handle)).toBe(true);
  expect(objectDetails(comparison, "impl").after?.contains).toEqual(["method"]);
});

it("does not guess type ownership when an implementation has ambiguous or external targets", async () => {
  const a = diagramNode("A", "type"), b = diagramNode("B", "type"), method = diagramNode("method", "callable");
  const external: CodeReference = { semantic: "code.applies_to", target: { kind: "external", symbol: "outside-type" } };
  const unresolved: CodeReference = { semantic: "code.applies_to", target: { kind: "unresolved", name: "unknown-type" } };
  for (const references of [[relation("code.applies_to", "A"), relation("code.applies_to", "B")], [external], [relation("code.applies_to", "A"), external], [unresolved], [relation("code.applies_to", "A"), unresolved]]) {
    const implementation = diagramNode("impl", "extension", { contains: ["method"], references });
    const nodes = [a, b, implementation, method];
    const layout = await layoutStructureComparison(diagramComparison(nodes, nodes), { ...options, showAll: true });
    expect(entry(layout, "impl")!.row).toBeUndefined();
    expect(entry(layout, "method")!.card.id).toBe(entry(layout, "impl")!.card.id);
    expect(entry(layout, "impl")!.card.parentId).toBeUndefined();
  }
});

it("keeps nested declarations reachable and expands hidden class methods without reanalysis", async () => {
  const comparison = structureReviewFixture();
  const initial = await layoutStructureComparison(comparison, options);
  expect(entry(initial, "restore")).toBeUndefined();
  const expanded = await layoutStructureComparison(comparison, { ...options, expanded: new Set(["Review"]) });
  expect(entry(expanded, "restore")?.row?.kind).toBe("callable");
  expect(entry(expanded, "review-id")?.row?.kind).toBe("value");
  expect(expanded.total).toBe(initial.total);
  const nodes = [diagramNode("outer", "callable", { contains: ["Inner", "helper"] }), diagramNode("Inner", "type", { contains: ["field"] }), diagramNode("field", "value"), diagramNode("helper", "callable")];
  const nested = await layoutStructureComparison(diagramComparison(nodes, nodes), { ...options, showAll: true });
  expect(entry(nested, "Inner")!.card.parentId).toBe(entry(nested, "outer")!.card.id);
  expect(entry(nested, "field")!.card.id).toBe(entry(nested, "Inner")!.card.id);
  expect(entry(nested, "helper")!.card.parentId).toBe(entry(nested, "outer")!.card.id);
});

it("collects root functions in a compact declaration table without inventing a module identity", async () => {
  const nodes = [diagramNode("one", "callable"), diagramNode("two", "callable"), diagramNode("constant", "value")];
  const layout = await layoutStructureComparison(diagramComparison(nodes, nodes), { ...options, showAll: true });
  expect(layout.nodes).toHaveLength(1);
  expect(layout.nodes[0].data.table).toBe(true);
  expect(layout.nodes[0].data.objectId).toBeUndefined();
  expect(layout.nodes[0].data.rows.filter((row) => row.objectId).map((row) => row.objectId).sort()).toEqual(["constant", "one", "two"]);
  expect(layout.visible).toBe(3);
  expect(layout.total).toBe(3);
});

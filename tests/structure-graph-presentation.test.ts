import { describe, expect, it } from "vitest";
import { structureReviewFixture } from "@/dev/structure-review-fixture";
import { isReferenceVisible, layoutStructureComparison, objectDetails } from "@/structure-review/graph-presentation";

const options = { showAll: false, showUnchangedReferences: false, showImports: false, showTypeUses: false, showDependencies: false, expanded: new Set<string>() };

describe("Structure Graph presentation Interface", () => {
  it("keeps own content changes separate from relationship changes and includes containing context", async () => {
    const layout = await layoutStructureComparison(structureReviewFixture(), options);
    const object = (id: string) => layout.nodes.find((node) => node.data.objectId === id)!;
    expect(object("run").data.status).toBe("modified");
    expect(object("review").data.status).toBe("relations");
    expect(object("impl").data.status).toBe("unchanged");
    expect(object("new").data.status).toBe("added");
    expect(object("old").data.status).toBe("removed");
    expect(object("run").parentId).toBe(object("impl").id);
    expect(object("keep")).toBeUndefined();
    expect(object("storage").data.hiddenChildren).toBe(2);
    expect(layout.edges.some((edge) => edge.source === object("new").id && edge.target === object("save").id)).toBe(true);
    expect(layout.nodes.some((node) => node.data.targetKind === "external")).toBe(true);
    expect(layout.nodes.some((node) => node.data.targetKind === "unresolved")).toBe(true);
    for (const child of layout.nodes.filter((node) => node.parentId)) {
      const parent = layout.nodes.find((node) => node.id === child.parentId)!;
      expect(layout.nodes.indexOf(parent)).toBeLessThan(layout.nodes.indexOf(child));
      expect(child.position.x).toBeGreaterThanOrEqual(0);
      expect(child.position.y).toBeGreaterThanOrEqual(90);
      expect(child.position.x + Number(child.style?.width)).toBeLessThanOrEqual(Number(parent.style?.width));
      expect(child.position.y + Number(child.style?.height)).toBeLessThanOrEqual(Number(parent.style?.height));
    }
  });

  it("expands unchanged children without changing analysis scope", async () => {
    const comparison = structureReviewFixture();
    const layout = await layoutStructureComparison(comparison, { ...options, expanded: new Set(["storage"]) });
    expect(layout.nodes.find((node) => node.data.objectId === "long")?.data.label).toContain("very_long");
    expect(layout.nodes.some((node) => node.data.objectId === "keep")).toBe(false);
    const all = await layoutStructureComparison(comparison, { ...options, showAll: true, showUnchangedReferences: true });
    expect(all.visible).toBe(all.total);
    expect(all.edges.some((edge) => edge.label === "code.member_of")).toBe(true);
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

it("hides standard-library and third-party targets while preserving repository targets outside the selected graph", async () => {
  const comparison = structureReviewFixture();
  for (const overrides of [{}, { showAll: true, showUnchangedReferences: true }, { showImports: true }]) {
    const layout = await layoutStructureComparison(comparison, { ...options, ...overrides });
    expect(layout.nodes.some((node) => node.data.targetScope === "dependency")).toBe(false);
    expect(layout.nodes.find((node) => node.data.targetScope === "repository")?.data.label).toBe("verify_snapshot");
    const ids = new Set(layout.nodes.map((node) => node.id));
    expect(layout.edges.every((edge) => ids.has(edge.source) && ids.has(edge.target))).toBe(true);
  }
  const layout = await layoutStructureComparison(comparison, { ...options, showDependencies: true });
  expect(layout.nodes.filter((node) => node.data.targetScope === "dependency").map((node) => node.data.label).sort()).toEqual(["Arc<T>::new", "serde_json::to_string"]);
  expect(layout.nodes.some((node) => node.data.label.includes("%2F") || node.data.label.startsWith("rust:"))).toBe(false);
});

it("only includes import-only targets when imports are enabled", async () => {
  const comparison = structureReviewFixture();
  const hidden = await layoutStructureComparison(comparison, options);
  expect(hidden.nodes.some((node) => node.data.objectId === "load")).toBe(false);
  expect(hidden.edges.some((edge) => String(edge.label).includes("code.imports"))).toBe(false);
  const shown = await layoutStructureComparison(comparison, { ...options, showImports: true });
  expect(shown.nodes.some((node) => node.data.objectId === "load")).toBe(true);
  expect(shown.edges.some((edge) => edge.label === "+ code.imports")).toBe(true);
});

it("omits Rust use declarations and incident relations from every display mode without changing the analysis", async () => {
  const comparison = structureReviewFixture();
  const useReference = { semantic: "code.references", target: { kind: "node" as const, id: "use-new" } };
  comparison.after.nodes.find((node) => node.id === "new")!.references.push(useReference);
  comparison.diff.nodes.find((node) => node.id === "new")!.references.push({ kind: "added", value: useReference });
  const original = structuredClone(comparison);
  for (const overrides of [{}, { expanded: new Set(["review", "storage"]) }, { showAll: true, showImports: true, showDependencies: true, showUnchangedReferences: true }]) {
    const layout = await layoutStructureComparison(comparison, { ...options, showTypeUses: true, ...overrides });
    expect(layout.nodes.some((node) => node.data.nativeKind === "use" || node.data.label === "imported_but_unused")).toBe(false);
    expect(layout.changed).toBe(4);
    expect(layout.total).toBe(12);
    const source = layout.nodes.find((node) => node.data.objectId === "new")!;
    const target = layout.nodes.find((node) => node.data.objectId === "Review")!;
    expect(layout.edges.some((edge) => edge.source === source.id && edge.target === target.id && edge.label === "+ code.type_use")).toBe(true);
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

it("hides type-use relationships and their exclusive targets until explicitly enabled", async () => {
  const comparison = structureReviewFixture();
  const original = structuredClone(comparison);
  for (const overrides of [{}, { showAll: true, showUnchangedReferences: true }, { showImports: true, showDependencies: true }]) {
    const displayOptions = { ...options, ...overrides };
    const hidden = await layoutStructureComparison(comparison, displayOptions);
    expect(hidden.edges.some((edge) => String(edge.label).includes("code.type_use"))).toBe(false);
    expect(hidden.nodes.some((node) => node.data.label === "SnapshotOptions")).toBe(false);
    expect(hidden.edges.some((edge) => String(edge.label).includes("code.calls"))).toBe(true);
    const details = objectDetails(comparison, "new");
    expect(details.after?.references.filter((reference) => isReferenceVisible(comparison, reference, displayOptions))
      .some((reference) => reference.semantic === "code.type_use")).toBe(false);
    expect(details.difference?.references.filter((change) => isReferenceVisible(comparison, change.value, displayOptions))
      .some((change) => change.value.semantic === "code.type_use")).toBe(false);
  }
  const displayOptions = { ...options, showTypeUses: true };
  const shown = await layoutStructureComparison(comparison, displayOptions);
  expect(shown.edges.filter((edge) => edge.label === "+ code.type_use")).toHaveLength(2);
  expect(shown.nodes.some((node) => node.data.label === "SnapshotOptions")).toBe(true);
  expect(shown.nodes.some((node) => node.data.targetScope === "dependency")).toBe(false);
  const reference = comparison.after.nodes.find((node) => node.id === "new")!.references.find((reference) => reference.semantic === "code.type_use")!;
  expect(isReferenceVisible(comparison, reference, displayOptions)).toBe(true);
  expect(comparison).toEqual(original);
});

it("uses declared symbol scope and names instead of interpreting opaque identity strings", async () => {
  const comparison = structureReviewFixture();
  const reference = { semantic: "code.calls", target: { kind: "external" as const, symbol: "opaque:%2F" } };
  comparison.after.nodes.find((node) => node.id === "new")!.references = [reference];
  comparison.diff.nodes.find((node) => node.id === "new")!.references = [{ kind: "added", value: reference }];
  comparison.after.externalSymbols[reference.target.symbol] = { name: "工具::处理", scope: "repository" };
  const shown = await layoutStructureComparison(comparison, options);
  expect(shown.nodes.some((node) => node.data.label === "工具::处理")).toBe(true);
  comparison.after.externalSymbols[reference.target.symbol].scope = "dependency";
  const hidden = await layoutStructureComparison(comparison, options);
  expect(hidden.nodes.some((node) => node.data.label === "工具::处理")).toBe(false);
  delete comparison.after.externalSymbols[reference.target.symbol];
  const unknown = await layoutStructureComparison(comparison, options);
  expect(unknown.nodes.some((node) => node.data.targetKind === "external" && node.data.targetScope === undefined)).toBe(true);
  expect(unknown.nodes.some((node) => node.data.label === reference.target.symbol)).toBe(false);
});

it("retains descriptions for removed references from the before graph", async () => {
  const comparison = structureReviewFixture();
  const reference = { semantic: "code.calls", target: { kind: "external" as const, symbol: "old-opaque-symbol" } };
  comparison.before.nodes.find((node) => node.id === "old")!.references = [reference];
  comparison.diff.nodes.find((node) => node.id === "old")!.references = [{ kind: "removed", value: reference }];
  comparison.before.externalSymbols[reference.target.symbol] = { name: "旧模块::旧函数", scope: "repository" };
  const layout = await layoutStructureComparison(comparison, options);
  const target = layout.nodes.find((node) => node.data.label === "旧模块::旧函数")!;
  expect(layout.edges.some((edge) => edge.target === target.id && edge.label === "− code.calls")).toBe(true);
});

it("stops abandoned layout requests through the public Interface", async () => {
  const abort = new AbortController();
  const running = layoutStructureComparison(structureReviewFixture(), options, abort.signal);
  abort.abort();
  await expect(running).rejects.toMatchObject({ name: "AbortError" });
});

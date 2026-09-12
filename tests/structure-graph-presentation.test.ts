import { describe, expect, it } from "vitest";
import { structureReviewFixture } from "@/dev/structure-review-fixture";
import { layoutStructureComparison } from "@/structure-review/graph-presentation";

const options = { showAll: false, showUnchangedReferences: false, expanded: new Set<string>() };

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
    comparison.before = { root: ["a"], nodes: [a, b] };
    comparison.after = { root: ["b"], nodes: [{ ...a, contains: [] }, { ...b, contains: ["a"] }] };
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

it("stops abandoned layout requests through the public Interface", async () => {
  const abort = new AbortController();
  const running = layoutStructureComparison(structureReviewFixture(), options, abort.signal);
  abort.abort();
  await expect(running).rejects.toMatchObject({ name: "AbortError" });
});

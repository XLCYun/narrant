import { describe, expect, it } from "vitest";
import { createStructureGraphPresentation, buildStructureGraph, expandStructureNeighbors } from "@/review/structure-graph";
import { createStructurePreviewResult } from "@/dev/scenarios/structure-preview-data";
import { structureContractPreview } from "@/dev/scenarios/structure-fixture";

describe("Structure graph topology", () => {
  it("keeps initial context and relationship-only endpoints discoverable while default lines are hidden", () => {
    const result = structuredClone(structureContractPreview);
    result.relationships[0].kind = "type-reference";
    result.relationships[0].change = "added";
    const graph = buildStructureGraph(result, createStructureGraphPresentation(result));
    expect(graph.nodes.map((node) => node.id)).toEqual(expect.arrayContaining(["decl:welcome", "decl:helper", "external:sonner:toast"]));
    expect(graph.relationships).toHaveLength(0);
    expect(graph.relationshipChanges.map((edge) => edge.id)).toEqual(["relation:welcome:toast:calls"]);
  });
  it("searches the complete analysis beyond the first batch", () => {
    const result = structuredClone(structureContractPreview);
    result.initialFocus.nodeIds = ["decl:helper"];
    const state = createStructureGraphPresentation(result);
    state.displayLimit = 1;
    state.query = "welcome";
    expect(buildStructureGraph(result, state).searchResults.map((node) => node.id)).toContain("decl:welcome");
  });
  it("expands distinct before-only and after-only callers and callees in the requested direction", () => {
    const result = structuredClone(structureContractPreview);
    const declaration = result.nodes.find((node) => node.id === "decl:helper");
    if (declaration?.kind !== "declaration" || declaration.after.state !== "present") throw new Error("Fixture requires a declaration");
    const value = declaration.after.value;
    for (const id of ["old-caller", "new-caller", "old-callee", "new-callee"]) {
      result.nodes.push({ ...declaration, id, changes: [],
        before: { state: "present", value: { ...value, name: id, locations: value.locations.map((location) => ({ ...location, side: "before" })) } },
        after: { state: "present", value: { ...value, name: id } } });
    }
    const edge = result.relationships[0];
    if (edge.after.state !== "present") throw new Error("Fixture requires an occurrence");
    const afterOccurrences = edge.after.value;
    const beforeOccurrences = afterOccurrences.map((occurrence) => ({ ...occurrence, location: { ...occurrence.location, side: "before" as const } }));
    result.relationships = [
      { ...edge, id: "old-upstream", fromId: "old-caller", toId: "decl:welcome", change: "deleted", before: { state: "present", value: beforeOccurrences }, after: { state: "absent" } },
      { ...edge, id: "new-upstream", fromId: "new-caller", toId: "decl:welcome", change: "added", before: { state: "absent" }, after: { state: "present", value: afterOccurrences } },
      { ...edge, id: "old-downstream", fromId: "decl:welcome", toId: "old-callee", change: "deleted", before: { state: "present", value: beforeOccurrences }, after: { state: "absent" } },
      { ...edge, id: "new-downstream", fromId: "decl:welcome", toId: "new-callee", change: "added", before: { state: "absent" }, after: { state: "present", value: afterOccurrences } },
    ];
    result.initialFocus = { nodeIds: ["decl:welcome"], relationshipIds: [] };
    const state = createStructureGraphPresentation(result);
    state.selectedNodeId = "decl:welcome";
    const upstream = buildStructureGraph(result, expandStructureNeighbors(result, state, "upstream"));
    expect(upstream.nodes.map((node) => node.id)).toEqual(["file:welcome", "decl:welcome", "old-caller", "new-caller"]);
    expect(upstream.relationships.map(({ relationship }) => relationship.id)).toEqual(["old-upstream", "new-upstream"]);
    const downstream = buildStructureGraph(result, expandStructureNeighbors(result, state, "downstream"));
    expect(downstream.nodes.map((node) => node.id)).toEqual(["file:welcome", "decl:welcome", "old-callee", "new-callee"]);
    expect(downstream.relationships.map(({ relationship }) => relationship.id)).toEqual(["old-downstream", "new-downstream"]);
  });
  it("does not fill the initial focus with unrelated declarations from expanded file containers", () => {
    const result = structuredClone(structureContractPreview);
    result.initialFocus.nodeIds = ["decl:helper"];
    expect(buildStructureGraph(result, createStructureGraphPresentation(result)).nodes.map((node) => node.id)).not.toContain("decl:welcome");
  });
  it("opens relation-only endpoint containers and preserves a single member identity inside native impl nesting", () => {
    const result = createStructurePreviewResult("relationship-only");
    const graph = buildStructureGraph(result, createStructureGraphPresentation(result));
    const method = graph.nodes.find((node) => node.id === "decl:refresh");
    expect(method?.parentId).toBe("impl:lifecycle");
    expect(graph.nodes.find((node) => node.id === "impl:lifecycle")?.parentId).toBe("decl:owner");
    expect(graph.nodes.filter((node) => node.id === "decl:refresh")).toHaveLength(1);
  });
  it("counts required hierarchy once and allows revealed members to collapse again", () => {
    const result = createStructurePreviewResult();
    const state = createStructureGraphPresentation(result);
    state.query = "refresh";
    const searched = buildStructureGraph(result, state);
    expect(searched.visibleCount).toBe(3);
    expect(searched.eligibleCount).toBe(3);
    state.query = "";
    state.revealedNodeIds = ["decl:refresh"];
    state.expandedIds = ["file:welcome"];
    expect(buildStructureGraph(result, state).nodes.map((node) => node.id)).not.toContain("decl:refresh");
  });
  it("separates relationship kind, certainty, selected-call range and external visibility", () => {
    const result = createStructurePreviewResult();
    const state = createStructureGraphPresentation(result);
    const edgeIds = () => buildStructureGraph(result, state).relationships.map(({ relationship }) => relationship.id);
    expect(edgeIds()).toEqual(["relation:owner:helper"]);
    state.selectedNodeId = "decl:refresh";
    expect(edgeIds()).toEqual(["relation:refresh:welcome", "relation:owner:helper"]);
    state.filters.calls = "graph";
    expect(edgeIds()).toContain("relation:welcome:toast:calls");
    state.filters.external = false;
    expect(edgeIds()).not.toContain("relation:welcome:toast:calls");
    result.relationships[2].certainty = "candidate";
    expect(edgeIds()).not.toContain("relation:owner:helper");
    state.filters.candidates = true;
    expect(edgeIds()).toContain("relation:owner:helper");
    state.filters.calls = "hidden";
    state.filters.implementations = false;
    expect(edgeIds()).toEqual([]);
  });
  it("toggles import and type-reference lines independently without hiding their change entries or counts", () => {
    const result = createStructurePreviewResult("relationship-controls");
    const importEdge = result.relationships.find((edge) => edge.id === "relation:welcome:dependency:imports");
    if (!importEdge) throw new Error("Fixture requires the file-to-file import relationship");
    result.relationships.push({ ...importEdge, id: "relation:welcome:dependency:type", fromId: "decl:welcome", toId: "decl:dependency", kind: "type-reference" });
    result.initialFocus.relationshipIds.push("relation:welcome:dependency:type");
    result.summary.relationshipChanges = 4;
    const state = createStructureGraphPresentation(result);
    const configurations = [
      { imports: false, types: false, visibleIds: [] },
      { imports: true, types: false, visibleIds: ["relation:welcome:dependency:imports"] },
      { imports: false, types: true, visibleIds: ["relation:welcome:dependency:type"] },
      { imports: true, types: true, visibleIds: ["relation:welcome:dependency:imports", "relation:welcome:dependency:type"] },
    ];
    expect(state.filters.imports).toBe(false);
    expect(state.filters.typeReferences).toBe(false);
    for (const configuration of configurations) {
      state.filters.imports = configuration.imports;
      state.filters.typeReferences = configuration.types;
      const graph = buildStructureGraph(result, state);
      expect(graph.relationships.filter(({ relationship }) => relationship.kind === "imports" || relationship.kind === "type-reference").map(({ relationship }) => relationship.id)).toEqual(configuration.visibleIds);
      expect(graph.relationshipChanges).toHaveLength(4);
      expect(graph.relationshipChanges.map((edge) => edge.id)).toEqual([
        "relation:refresh:welcome", "relation:owner:helper", "relation:welcome:dependency:imports", "relation:welcome:dependency:type",
      ]);
      expect(graph.nodes.map((node) => node.id)).toEqual(expect.arrayContaining(["file:welcome", "file:dependency", "decl:welcome", "decl:dependency"]));
    }
  });
});

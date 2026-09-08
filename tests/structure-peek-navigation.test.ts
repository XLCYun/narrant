import { describe, expect, it } from "vitest";
import { structureContractPreview } from "@/dev/scenarios/structure-fixture";
import { resolveStructurePeekNavigation } from "@/review/structure-peek";

describe("Structure Peek 源码导航", () => {
  it("关系先定位发生位置，目标定义保留为独立导航动作", () => {
    const selection = resolveStructurePeekNavigation(structureContractPreview, {
      kind: "relationship", relationshipId: "relation:welcome:toast:calls",
    });
    expect(selection?.locations[0]).toMatchObject({ side: "after", role: "occurrence", range: { startLine: 3 } });
    expect(selection?.targetDefinition).toEqual({ kind: "target-definition", nodeId: "external:sonner:toast", relationshipId: "relation:welcome:toast:calls" });
    const definition = resolveStructurePeekNavigation(structureContractPreview, selection!.targetDefinition!);
    expect(definition?.unavailableReason).toBe("external-source-not-indexed");
    expect(definition?.locations).toEqual([]);
  });

  it("跨文件移动分别保留两个真实文件对，默认新位置，删除默认旧位置", () => {
    const result = structuredClone(structureContractPreview);
    const node = result.nodes.find((item) => item.id === "decl:welcome")!;
    if (node.kind !== "declaration" || node.after.state !== "present" || node.before.state !== "present") throw new Error("fixture");
    result.sources.push({ id: "moved-source", before: { state: "present", path: "destination.ts", revision: "base" }, after: { state: "present", path: "destination.ts", revision: "head" }, classification: { kind: "file-change", fileChange: { path: "destination.ts", oldPath: null, status: "modified" } } });
    node.changes = ["moved"];
    node.after.value.locations[0].sourceId = "moved-source";
    const moved = resolveStructurePeekNavigation(result, { kind: "node", nodeId: node.id });
    expect(moved?.locations.map((item) => [item.sourceId, item.side])).toEqual([["moved-source", "after"], ["preview:welcome.ts", "before"]]);
    node.after = { state: "absent" };
    node.changes = ["deleted"];
    expect(resolveStructurePeekNavigation(result, { kind: "node", nodeId: node.id })?.locations[0].side).toBe("before");
  });

  it("聚合类型默认声明文件，附加跨文件 impl，成员只进入自身来源", () => {
    const result = structuredClone(structureContractPreview);
    const node = result.nodes.find((item) => item.id === "decl:welcome")!;
    if (node.kind !== "declaration" || node.after.state !== "present") throw new Error("fixture");
    for (const side of [node.before, node.after]) {
      if (side.state === "present") Object.assign(side.value, { nativeKind: { language: "rust", category: "struct", name: "struct" }, name: "Welcome", qualifiedName: "Welcome", signature: "pub struct Welcome" });
    }
    result.sources[0] = { id: "preview:welcome.ts", before: { state: "present", path: "types/welcome.rs", revision: "base-types" }, after: { state: "present", path: "types/welcome.rs", revision: "head-types" }, classification: { kind: "file-change", fileChange: { path: "types/welcome.rs", oldPath: null, status: "modified" } } };
    result.sources.push({ id: "impl-source", before: { state: "present", path: "impl/welcome.rs", revision: "base-impl" }, after: { state: "present", path: "impl/welcome.rs", revision: "head-impl" }, classification: { kind: "unchanged", path: "impl/welcome.rs" } });
    const implementation = { ...node.after.value.locations[0], sourceId: "impl-source", role: "implementation" as const, range: { startLine: 20, endLine: 28, startColumn: 1, endColumn: 2 } };
    const implValue = { ownerId: node.id, traitName: null, traitTargetId: null, signature: "impl Welcome", locations: [implementation] };
    result.implementationGroups.push({ id: "impl", before: { state: "present", value: { ...implValue, locations: [{ ...implementation, side: "before" }] } }, after: { state: "present", value: implValue } });
    const member = result.nodes.find((item) => item.id === "decl:helper")!;
    if (member.kind !== "declaration" || member.after.state !== "present") throw new Error("member fixture");
    Object.assign(member.after.value, { parentId: node.id, implementationGroupId: "impl", nativeKind: { language: "rust", category: "method", name: "method" }, signature: "pub fn helper(&self)", locations: [{ ...implementation, role: "declaration", range: { startLine: 25, endLine: 27, startColumn: 3, endColumn: 4 } }] });
    member.before = { state: "present", value: { ...member.after.value, locations: member.after.value.locations.map((location) => ({ ...location, side: "before" })) } };
    member.changes = [];
    const selection = resolveStructurePeekNavigation(result, { kind: "node", nodeId: node.id });
    expect(selection?.locations[0]).toMatchObject({ sourceId: "preview:welcome.ts", role: "declaration" });
    const implLocation = selection!.locations.find((location) => location.role === "implementation" && location.side === "after")!;
    expect(implLocation).toEqual(implementation);
    const declarationNavigation = resolveStructurePeekNavigation(result, { kind: "location", location: selection!.locations[0] })!;
    const implNavigation = resolveStructurePeekNavigation(result, { kind: "location", location: implLocation })!;
    expect(declarationNavigation.locations[0].sourceId).toBe("preview:welcome.ts");
    expect(declarationNavigation.sources.find((source) => source.id === declarationNavigation.locations[0].sourceId)).toMatchObject({ before: { path: "types/welcome.rs" }, after: { path: "types/welcome.rs" } });
    expect(implNavigation.locations[0].sourceId).toBe("impl-source");
    expect(implNavigation.sources.find((source) => source.id === implNavigation.locations[0].sourceId)).toMatchObject({ before: { path: "impl/welcome.rs" }, after: { path: "impl/welcome.rs" }, classification: { kind: "unchanged" } });
    expect(resolveStructurePeekNavigation(result, { kind: "node", nodeId: "decl:helper" })?.locations).toEqual([
      { sourceId: "impl-source", side: "after", role: "declaration", range: { startLine: 25, endLine: 27, startColumn: 3, endColumn: 4 } },
      { sourceId: "impl-source", side: "before", role: "declaration", range: { startLine: 25, endLine: 27, startColumn: 3, endColumn: 4 } },
    ]);
  });

  it("多个发生位置保留每一侧与原始范围，删除关系优先旧侧", () => {
    const result = structuredClone(structureContractPreview);
    const edge = result.relationships[0];
    if (edge.after.state !== "present") throw new Error("fixture");
    const occurrence = edge.after.value[0];
    edge.before = { state: "present", value: [{ ...occurrence, location: { ...occurrence.location, side: "before", range: { startLine: 2, startColumn: 3, endLine: 2, endColumn: 18 } } }] };
    edge.after.value.push({ ...occurrence, location: { ...occurrence.location, range: { startLine: 4, startColumn: 3, endLine: 4, endColumn: 19 } } });
    edge.change = "deleted";
    expect(resolveStructurePeekNavigation(result, { kind: "relationship", relationshipId: edge.id })?.locations.map((item) => [item.side, item.range.startLine])).toEqual([["before", 2], ["after", 3], ["after", 4]]);
  });
});

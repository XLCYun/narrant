import { expect, it } from "vitest";
import { structureMixedLanguagesFixture } from "@/dev/structure-review-fixture";
import { comparisonForLanguage, structureLanguages } from "@/structure-review/language-view";

it("discovers languages on both sides, including languages whose objects were all removed", () => {
  const comparison = structureMixedLanguagesFixture();
  comparison.after.nodes = comparison.after.nodes.filter((node) => node.language !== "typescript");
  comparison.before.nodes.push({ ...comparison.before.nodes[0], id: "future", language: "future-language" });
  expect(structureLanguages(comparison)).toEqual(["future-language", "rust", "typescript"]);
  expect(comparisonForLanguage(comparison, "typescript").before.nodes).toHaveLength(5);
  expect(comparisonForLanguage(comparison, "typescript").after.nodes).toHaveLength(0);
});

it("keeps graph, relation differences and details within the selected language without mutating analysis", () => {
  const comparison = structureMixedLanguagesFixture();
  const original = structuredClone(comparison);
  const view = comparisonForLanguage(comparison, "typescript");
  expect(view.before.nodes.every((node) => node.language === "typescript")).toBe(true);
  expect(view.after.nodes.every((node) => node.language === "typescript")).toBe(true);
  expect(view.after.nodes.find((node) => node.id === "ts-refresh")?.references).toEqual([
    { semantic: "code.calls", target: { kind: "node", id: "ts-persist" } },
  ]);
  expect(view.diff.nodes.map((node) => node.id)).toEqual(["ts-title", "ts-refresh"]);
  expect(view.diff.nodes.find((node) => node.id === "ts-refresh")?.references).toHaveLength(1);
  expect(view.after.root).toEqual(["ts-module"]);
  expect(comparison).toEqual(original);
});

it("promotes objects with a filtered parent and removes changes that only connect other languages", () => {
  const comparison = structureMixedLanguagesFixture();
  comparison.after.nodes.find((node) => node.id === "ts-module")!.contains.push("Group");
  comparison.diff.nodes.push({ id: "ts-module", content: null, contains: [{ kind: "added", value: "Group" }], references: [] });
  const rust = comparisonForLanguage(comparison, "rust");
  expect(rust.after.root).toEqual(["github-discovery"]);
  expect(comparisonForLanguage(comparison, "typescript").diff.nodes.map((node) => node.id)).not.toContain("ts-module");
  comparison.after.nodes.find((node) => node.id === "github-discovery")!.contains = [];
  expect(comparisonForLanguage(comparison, "rust").after.root).toContain("Group");
});

import { describe, expect, it } from "vitest";

import { buildBabysitZoneRows, setBabysitExpansion, syncInlineDeletedExplanations } from "@/components/babysit-diff-editor";

/** 构造 Monaco change shape，覆盖 add/delete/unequal 的配对规则。 */
function change(originalStart: number, originalEnd: number, modifiedStart: number, modifiedEnd: number) {
  return { originalStartLineNumber: originalStart, originalEndLineNumber: originalEnd, modifiedStartLineNumber: modifiedStart, modifiedEndLineNumber: modifiedEnd };
}

describe("Babysit Monaco zone pairing", () => {
  it("同一次 click 的 desired state 被重复处理也保持幂等", () => {
    let expanded = new Set<string>();
    expanded = setBabysitExpansion(expanded, "before:2", true);
    expanded = setBabysitExpansion(expanded, "before:2", true);
    expect([...expanded]).toEqual(["before:2"]);
    expanded = setBabysitExpansion(expanded, "before:2", false);
    expanded = setBabysitExpansion(expanded, "before:2", false);
    expect(expanded).toHaveLength(0);
  });

  it("为 add-only 的 before 侧保留空配对信息", () => {
    const rows = buildBabysitZoneRows([], [[2, "新增一"], [3, "新增二"]], [change(0, 0, 2, 3)]);
    expect(rows).toHaveLength(2);
    expect(rows.map((row) => row.before)).toEqual([undefined, undefined]);
    expect(rows.map((row) => row.afterAnchor)).toEqual([2, 3]);
  });

  it("为 delete-only 的 after 侧保留空配对信息", () => {
    const rows = buildBabysitZoneRows([[2, "删除一"], [3, "删除二"]], [], [change(2, 3, 0, 0)]);
    expect(rows).toHaveLength(2);
    expect(rows.map((row) => row.after)).toEqual([undefined, undefined]);
    expect(rows.map((row) => row.beforeAnchor)).toEqual([2, 3]);
  });

  it("不等长修改取较大侧行数且每行保持 paired anchors", () => {
    const rows = buildBabysitZoneRows([[2, "旧"]], [[2, "新一"], [3, "新二"], [4, "新三"]], [change(2, 2, 2, 4)]);
    expect(rows).toHaveLength(3);
    expect(rows[0]).toMatchObject({ beforeAnchor: 2, afterAnchor: 2 });
    expect(rows.slice(1).every((row) => row.before === undefined)).toBe(true);
  });

  it("把 original 解释精确挂到单栏红色删除行", () => {
    const root = document.createElement("div");
    root.innerHTML = '<div class="view-zones"><div class="line-delete"><div class="view-line"><span>旧行二</span></div><div class="view-line"><span>旧行三</span></div></div></div>';

    syncInlineDeletedExplanations(root, [[3, "解释被删除的第三行。"]], [change(2, 3, 2, 3)]);

    const rows = root.querySelectorAll(".view-line");
    expect(rows[0]?.querySelector("[data-babysit-inline-deleted]")).toBeNull();
    expect(rows[1]?.querySelector("[data-babysit-inline-deleted]")).toHaveTextContent("解释被删除的第三行。");
    expect(rows[1]).not.toHaveTextContent("变更前 · 3");
  });
});

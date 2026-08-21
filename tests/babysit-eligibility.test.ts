import { describe, expect, it } from "vitest";

import { getBabysitEligibility } from "@/babysit/eligibility";

describe("Babysit file eligibility", () => {
  it("只允许首批代码语言并跳过 lock、生成物和依赖目录", () => {
    expect(getBabysitEligibility("src/example.ts", { contentKind: "text" })).toEqual({ eligible: true });
    expect(getBabysitEligibility("pnpm-lock.yaml", { contentKind: "text" })).toEqual({ eligible: false, reason: "lock" });
    expect(getBabysitEligibility("src/example.min.ts", { contentKind: "text" })).toEqual({ eligible: false, reason: "minified" });
    expect(getBabysitEligibility("generated/client.ts", { contentKind: "text" })).toEqual({ eligible: false, reason: "generated" });
    expect(getBabysitEligibility("node_modules/pkg/index.ts", { contentKind: "text" })).toEqual({ eligible: false, reason: "dependency" });
    expect(getBabysitEligibility("docs/example.ts", { contentKind: "text" })).toEqual({ eligible: false, reason: "generated" });
    expect(getBabysitEligibility("src/example.json", { contentKind: "text" })).toEqual({ eligible: false, reason: "unsupported-extension" });
  });

  it("禁用非文本 File Change 并报告具体类别", () => {
    expect(getBabysitEligibility("image.png", { contentKind: "binary" })).toEqual({ eligible: false, reason: "binary" });
    expect(getBabysitEligibility("module", { contentKind: "submodule" })).toEqual({ eligible: false, reason: "submodule" });
    expect(getBabysitEligibility("link", { contentKind: "symlink" })).toEqual({ eligible: false, reason: "symlink" });
    expect(getBabysitEligibility("src/example.ts")).toEqual({ eligible: false, reason: "missing-content" });
  });
});

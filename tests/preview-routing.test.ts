import { describe, expect, it } from "vitest";

import { resolvePreviewScenarioKey } from "@/dev/preview-routing";

describe("Preview Harness routing", () => {
  it("保留组件已声明的请求场景", () => {
    expect(resolvePreviewScenarioKey({ available: {}, staged: {} }, "staged")).toBe("staged");
  });

  it("未知场景回退到组件第一个场景而不假定存在 default", () => {
    expect(resolvePreviewScenarioKey({ available: {}, staged: {} }, "default")).toBe("available");
  });

  it("空组件定义直接失败并给出稳定诊断", () => {
    expect(() => resolvePreviewScenarioKey({}, "default")).toThrow("预览组件必须至少声明一个场景");
  });
});

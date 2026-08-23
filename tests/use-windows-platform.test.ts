import { describe, expect, it } from "vitest";
import { detectWindowsPlatform } from "@/hooks/use-windows-platform";

describe("Windows 平台探测", () => {
  it("优先使用 User-Agent Client Hints 识别 Windows", () => {
    expect(detectWindowsPlatform({
      userAgentData: { platform: "Windows" },
      platform: "MacIntel",
      userAgent: "Mozilla/5.0 (Macintosh)",
    })).toBe(true);
  });

  it("兼容传统 navigator.platform 的 Win32 标识", () => {
    expect(detectWindowsPlatform({
      platform: "Win32",
      userAgent: "Mozilla/5.0",
    })).toBe(true);
  });

  it("不会把 macOS 识别为 Windows", () => {
    expect(detectWindowsPlatform({
      platform: "MacIntel",
      userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
    })).toBe(false);
  });
});

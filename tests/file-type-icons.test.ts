import { describe, expect, it } from "vitest";

import { resolveFileTypeIcon } from "@/lib/file-type-icons";

describe("resolveFileTypeIcon", () => {
  it("优先匹配精确文件名和最长复合扩展名", () => {
    expect(resolveFileTypeIcon({ path: "Dockerfile", kind: "file", theme: "dark" }).name).toBe("docker");
    expect(resolveFileTypeIcon({ path: "src/button.test.ts", kind: "file", theme: "dark" }).name).toBe("test-ts");
  });

  it("为 dotfile、未知文件和目录展开状态提供稳定 fallback", () => {
    expect(resolveFileTypeIcon({ path: ".gitignore", kind: "file", theme: "dark" }).name).toBe("git");
    expect(resolveFileTypeIcon({ path: "assets/unknown.fixture", kind: "file", theme: "light" }).name).toBe("file");
    expect(resolveFileTypeIcon({ path: "assets/animation.blink", kind: "file", theme: "light" }).name).toBe("blink_light");
    expect(resolveFileTypeIcon({ path: "src", kind: "directory", expanded: false, theme: "dark" }).name).toBe("folder-src");
    expect(resolveFileTypeIcon({ path: "src", kind: "directory", expanded: true, theme: "dark" }).name).toBe("folder-src-open");
  });
});

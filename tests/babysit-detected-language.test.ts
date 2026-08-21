import { describe, expect, it } from "vitest";

import { detectBabysitLanguage } from "@/babysit/detected-language";
import type { FileChangeContent } from "@/review/types";

/** 构造 detector 使用的最小文本 File Change。 */
function text(path: string, source: string): FileChangeContent {
  return { path, oldPath: null, status: "added", contentKind: "text", beforeSize: null, afterSize: source.length, before: null, after: source };
}

describe("Babysit detected language", () => {
  it("识别白名单扩展名和可靠的 extensionless shebang", () => {
    expect(detectBabysitLanguage("src/app.tsx", text("src/app.tsx", "export {}"))).toBe("typescript");
    expect(detectBabysitLanguage("src/app.mts", text("src/app.mts", "export {}"))).toBe("typescript");
    expect(detectBabysitLanguage("src/app.mjs", text("src/app.mjs", "export {}"))).toBe("javascript");
    expect(detectBabysitLanguage("bin/tool", text("bin/tool", "#!/usr/bin/env python3\nprint('ok')"))).toBe("python");
    expect(detectBabysitLanguage("bin/task", text("bin/task", "#!/usr/bin/env node\nconsole.log('ok')"))).toBe("javascript");
  });

  it("未知 extensionless、Dockerfile 与 shell 不进入首批白名单", () => {
    expect(detectBabysitLanguage("bin/tool", text("bin/tool", "echo maybe"))).toBeUndefined();
    expect(detectBabysitLanguage("Dockerfile", text("Dockerfile", "FROM node:22"))).toBeUndefined();
    expect(detectBabysitLanguage("bin/tool", text("bin/tool", "#!/bin/sh\necho no"))).toBeUndefined();
  });
});

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("File Type Icon 构建资产", () => {
  it("校验可达资产并在超过预算时只输出警告", () => {
    const result = spawnSync("node", ["scripts/generate-file-type-icons.mjs"], {
      cwd: process.cwd(),
      env: { ...process.env, FILE_TYPE_ICON_MAX_BYTES: "1" },
      encoding: "utf8",
    });

    expect(result.status).toBe(0);
    expect(result.stdout).toMatch(/File Type Icons: \d+ SVGs, \d+ raw bytes/);
    expect(result.stderr).toContain("WARNING:");
    expect(existsSync(resolve(process.cwd(), "public/file-type-icons/file.svg"))).toBe(true);
  });
});

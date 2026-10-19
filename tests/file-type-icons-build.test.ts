import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("File Type Icon 构建资产", () => {
  it("校验可达资产并在超过预算时只输出警告", () => {
    const emptyIconsDirectory = mkdtempSync(resolve(tmpdir(), "narrant-file-icons-"));
    const missingAsset = spawnSync("node", ["scripts/dev/generate-file-type-icons.mjs"], {
      cwd: process.cwd(),
      env: { ...process.env, FILE_TYPE_ICON_SOURCE_DIRECTORY: emptyIconsDirectory },
      encoding: "utf8",
    });
    rmSync(emptyIconsDirectory, { recursive: true, force: true });
    expect(missingAsset.status).not.toBe(0);
    expect(missingAsset.stderr).toContain("Missing Material Icon Theme asset");

    const result = spawnSync("node", ["scripts/dev/generate-file-type-icons.mjs"], {
      cwd: process.cwd(),
      env: { ...process.env, FILE_TYPE_ICON_MAX_BYTES: "1" },
      encoding: "utf8",
    });

    expect(result.status).toBe(0);
    expect(result.stdout).toMatch(/File Type Icons: \d+ SVGs, \d+ raw bytes/);
    expect(result.stderr).toContain("WARNING:");
    expect(existsSync(resolve(process.cwd(), "public/file-type-icons/file.svg"))).toBe(true);
    expect(existsSync(resolve(process.cwd(), "public/file-type-icons/blink_light.svg"))).toBe(true);
    expect(existsSync(resolve(process.cwd(), "public/third-party-licenses/material-icon-theme-MIT.txt"))).toBe(true);
  }, 15_000);
});

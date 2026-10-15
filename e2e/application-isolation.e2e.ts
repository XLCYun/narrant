import { basename } from "node:path";
import { BaseDirectory } from "@tauri-apps/api/path";

describe("同一桌面账户的应用数据隔离", () => {
  it("真实 Tauri 的配置、数据和缓存目录全部属于 E2E 身份", async () => {
    // embedded 驱动注入的是 core.invoke；通过实际 Rust 命令验证运行中的配置与目录。
    const observation = await browser.tauri.execute(async ({ core }, directories: number[]) => ({
      name: await core.invoke<string>("plugin:app|name"),
      identifier: await core.invoke<string>("plugin:app|identifier"),
      directories: await Promise.all(directories.map((directory) =>
        core.invoke<string>("plugin:path|resolve_directory", { directory }))),
    }), [BaseDirectory.AppConfig, BaseDirectory.AppData, BaseDirectory.AppLocalData, BaseDirectory.AppCache]);
    expect(observation.name).toBe("Narrant E2E");
    expect(observation.identifier).toBe("com.cyunlab.narrant.e2e");
    for (const directory of observation.directories) {
      expect(basename(directory)).toBe("com.cyunlab.narrant.e2e");
    }
  });
});

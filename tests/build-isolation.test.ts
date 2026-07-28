import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = process.cwd();

function read(rel: string): string {
  return readFileSync(resolve(root, rel), "utf8");
}

describe("生产构建隔离(验收条件 #7:测试专用插件与 endpoint 不进入生产构建)", () => {
  it("wdio 不是默认 Cargo feature,且为可选依赖", () => {
    const cargo = read("src-tauri/Cargo.toml");
    expect(cargo).toMatch(/^default\s*=\s*\[\s*\]/m);
    expect(cargo).toMatch(/wdio\s*=\s*\[[^\]]*dep:tauri-plugin-wdio/);
    expect(cargo).toMatch(/optional\s*=\s*true/);
  });

  it("wdio 插件仅在 #[cfg(feature = \"wdio\")] 下注册", () => {
    const lib = read("src-tauri/src/lib.rs");
    expect(lib).toMatch(/#\[cfg\(feature = "wdio"\)\]/);
    expect(lib).toMatch(/tauri_plugin_wdio::init\(\)/);
  });

  it("生产 Tauri 配置不授予任何 wdio 权限", () => {
    const prod = JSON.parse(read("src-tauri/tauri.conf.json"));
    expect(JSON.stringify(prod)).not.toMatch(/wdio/i);
  });

  it("默认 capability 不含 wdio 权限", () => {
    const def = JSON.parse(read("src-tauri/capabilities/default.json"));
    expect(JSON.stringify(def.permissions)).not.toMatch(/wdio/i);
  });

  it("生产与 E2E capability 只授予原生目录选择所需的 dialog open 权限", () => {
    const def = JSON.parse(read("src-tauri/capabilities/default.json"));
    const e2e = JSON.parse(read("src-tauri/tauri.e2e.conf.json"));
    expect(def.permissions).toContain("dialog:allow-open");
    expect(JSON.stringify(e2e)).toMatch(/dialog:allow-open/);
    expect(JSON.stringify(def.permissions)).not.toMatch(/dialog:(?:default|allow-save)/);
  });

  it("仅 E2E 配置授予 wdio 权限", () => {
    const e2e = JSON.parse(read("src-tauri/tauri.e2e.conf.json"));
    expect(JSON.stringify(e2e)).toMatch(/wdio:default/);
  });

  it("前端测试用 data-* endpoint 仅在 VITE_WDIO 下暴露", () => {
    const app = read("src/App.tsx");
    const home = read("src/components/home-page.tsx");
    expect(app).toMatch(/data-before=\{import\.meta\.env\.VITE_WDIO/);
    expect(app).toMatch(/data-after=\{import\.meta\.env\.VITE_WDIO/);
    expect(home).toMatch(/import\.meta\.env\.VITE_WDIO\s*===\s*"true"/);
    expect(home).toMatch(/__NARRANT_E2E_DIRECTORY__/);
    const i18n = read("src/lib/i18n.ts");
    expect(i18n).toMatch(/import\.meta\.env\.VITE_WDIO\s*===\s*"true"/);
    expect(i18n).toMatch(/__NARRANT_E2E_SET_LOCALE__/);
  });

  it("生产依赖与模块不再包含旧 GitHub code-content 实现", () => {
    const cargo = read("src-tauri/Cargo.toml");
    const changeContent = read("src-tauri/src/change_content/mod.rs");
    expect(cargo).not.toMatch(/reqwest\s*=.*blocking/);
    expect(changeContent).not.toMatch(/pub\s+mod\s+github/);
  });

  it("前后端契约不再暴露 Review Session 或 source-tagged union", () => {
    const backend = read("src-tauri/src/lib.rs");
    const frontend = read("src/App.tsx");
    expect(backend).not.toMatch(/ReviewSessionStore|sessionId|ReviewSourceResponse/);
    expect(frontend).not.toMatch(/sessionId|close_review_session|open_github_pull_request/);
    expect(frontend).not.toMatch(/source:\s*["'](?:local|github)["']/);
  });
});

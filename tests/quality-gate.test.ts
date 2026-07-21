import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { parse } from "yaml";

const workflowPath = resolve(process.cwd(), ".github/workflows/quality-gate.yml");

function loadWorkflow(): Record<string, unknown> {
  const text = readFileSync(workflowPath, "utf8");
  return parse(text) as Record<string, unknown>;
}

function steps(job: Record<string, unknown>): unknown {
  return JSON.stringify(job.steps ?? []);
}

describe("质量门禁 workflow(验收条件 #1/#2/#3/#4/#5/#6)", () => {
  const wf = loadWorkflow();
  const jobs = wf.jobs as Record<string, Record<string, unknown>>;

  it("#1 在 pull_request 目标为 main 且 opened/reopened/synchronize 时触发", () => {
    // `yaml` 1.2 keeps `on` as a string; guard against the YAML 1.1 boolean quirk.
    const on = (wf.on ?? wf["on"] ?? wf["true"]) as Record<string, unknown>;
    expect(on).toBeTruthy();
    const pr = on.pull_request as { branches?: string[]; types?: string[] };
    expect(pr).toBeTruthy();
    expect(pr.branches).toContain("main");
    expect(pr.types).toEqual(expect.arrayContaining(["opened", "reopened", "synchronize"]));
  });

  it("#2 运行 Rust 格式、静态检查、单元及 Git fixture 集成测试", () => {
    const rust = jobs.rust;
    expect(rust).toBeTruthy();
    const cmds = steps(rust);
    expect(cmds).toMatch(/cargo fmt/);
    expect(cmds).toMatch(/cargo clippy/);
    expect(cmds).toMatch(/cargo test/);
  });

  it("#3 运行 TypeScript 类型检查、Lint 与 React 组件测试", () => {
    const ts = jobs.typescript;
    expect(ts).toBeTruthy();
    const cmds = steps(ts);
    expect(cmds).toMatch(/pnpm lint/);
    expect(cmds).toMatch(/pnpm build/); // build runs tsc --noEmit (类型检查)
    expect(cmds).toMatch(/pnpm test:components/);
  });

  it("#4 E2E 不访问真实 GitHub 账号或仓库,声明离线/mock", () => {
    const e2e = jobs.e2e;
    expect(e2e).toBeTruthy();
    const cmds = JSON.stringify({ env: e2e.env, steps: e2e.steps });
    expect(cmds).toMatch(/NARRANT_E2E_NO_REAL_GITHUB/);
    // 不向应用注入真实 GitHub 凭证或登录真实账号
    expect(cmds).not.toMatch(/gh auth login/);
    expect(cmds).not.toMatch(/NARRANT_GITHUB_TOKEN/);
  });

  it("#5 macOS 与 Windows runner 使用 WebdriverIO 真实 Tauri E2E", () => {
    const e2e = jobs.e2e;
    const matrix = e2e.strategy?.matrix as { os?: string[] };
    expect(matrix.os).toEqual(expect.arrayContaining(["macos-latest", "windows-latest"]));
    expect(steps(e2e)).toMatch(/pnpm test:e2e/);
    const wdioConf = readFileSync(resolve(process.cwd(), "wdio.conf.ts"), "utf8");
    expect(wdioConf).toMatch(/@wdio\/tauri-service/);
  });

  it("#6 同一 PR 新提交取消旧运行", () => {
    const concurrency = wf.concurrency as { "cancel-in-progress"?: boolean };
    expect(concurrency).toBeTruthy();
    expect(concurrency["cancel-in-progress"]).toBe(true);
  });

  it("#6 合理缓存 pnpm 与 Cargo 依赖", () => {
    const all = JSON.stringify(wf.jobs);
    expect(all).toMatch(/Swatinem\/rust-cache/);
    expect(all).toMatch(/pnpm\/action-setup|cache: "?pnpm"?/);
  });
});

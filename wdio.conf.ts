import { execFile, spawn, type ChildProcess } from "node:child_process";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

const binary = `./src-tauri/target/debug/narrant${process.platform === "win32" ? ".exe" : ""}`;
const executeFile = promisify(execFile);
let githubMock: ChildProcess | undefined;
let localFixtureRoot: string | undefined;

export const config = {
  runner: "local",
  logLevel: "error",
  specs: ["./e2e/**/*.e2e.ts"],
  maxInstances: 1,
  waitforTimeout: 15000,
  services: [["@wdio/tauri-service", { appBinaryPath: binary, driverProvider: "embedded" }]],
  capabilities: [{ browserName: "tauri", "tauri:options": { application: binary } }],
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 120000 },
  onPrepare: async () => {
    // 为真实 Tauri IPC 创建隔离的 Core Git 仓库，并把路径传给测试 worker。
    localFixtureRoot = await mkdtemp(join(tmpdir(), "narrant-e2e-"));
    const repositoryPath = join(localFixtureRoot, "repository");
    const trustRepositoryPath = join(localFixtureRoot, "trust-repository");
    const fixture = JSON.parse((await executeFile(
      process.execPath,
      ["e2e/fixtures/setup-local-repo.mjs", repositoryPath],
    )).stdout.trim());
    await executeFile(process.execPath, ["e2e/fixtures/setup-local-repo.mjs", trustRepositoryPath]);
    process.env.NARRANT_E2E_REPOSITORY = await realpath(repositoryPath);
    process.env.NARRANT_E2E_TRUST_REPOSITORY = await realpath(trustRepositoryPath);
    process.env.NARRANT_DATABASE_PATH = join(localFixtureRoot, "narrant.sqlite3");
    process.env.NARRANT_GITHUB_API_URL = "http://127.0.0.1:18472";
    githubMock = spawn(process.execPath, ["e2e/fixtures/github-mock-server.mjs"], {
      stdio: "inherit",
      env: {
        ...process.env,
        NARRANT_E2E_BASE_SHA: fixture.baseSha,
        NARRANT_E2E_HEAD_SHA: fixture.headSha,
      },
    });
    for (let attempt = 0; attempt < 50; attempt += 1) {
      try { if ((await fetch("http://127.0.0.1:18472/health")).ok) return; } catch { /* retry */ }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error("GitHub mock server did not start");
  },
  onComplete: async () => {
    githubMock?.kill();
    // Windows 的 onComplete 早于 Tauri service 释放 SQLite handle，runner 退出后会回收临时目录。
    if (localFixtureRoot && process.platform !== "win32") {
      await rm(localFixtureRoot, { recursive: true, force: true });
    }
  }
};

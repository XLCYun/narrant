import { execFile, spawn, type ChildProcess } from "node:child_process";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

const binary = `./src-tauri/target/debug/narrant${process.platform === "win32" ? ".exe" : ""}`;
const executeFile = promisify(execFile);
let githubMock: ChildProcess | undefined;
let localFixtureRoot: string | undefined;

/** 终止 GitHub mock 并等待子进程退出，避免临时目录清理与进程收尾竞态。 */
async function stopGitHubMock() {
  const mock = githubMock;
  githubMock = undefined;
  if (!mock || mock.exitCode !== null || mock.signalCode !== null) return;

  const exited = new Promise<void>((resolve) => {
    mock.once("exit", () => resolve());
    mock.once("error", () => resolve());
  });
  if (!mock.kill()) return;
  await exited;
}

/** 删除本次 E2E fixture；初始化回滚时尚未启动 Tauri，可在 Windows 强制清理。 */
async function removeFixture(force = false) {
  const fixtureRoot = localFixtureRoot;
  localFixtureRoot = undefined;
  if (fixtureRoot && (force || process.platform !== "win32")) {
    await rm(fixtureRoot, { recursive: true, force: true });
  }
}

/** 在 WDIO service 启动应用前准备并导出整套 E2E 隔离环境。 */
async function prepareE2EEnvironment() {
  // Worker 会继承 launcher 的环境；避免每个 worker 重建 fixture 或重复占用 mock 端口。
  if (process.env.NARRANT_E2E_PREPARED === "true") return;

  try {
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
    // Git trace2 由真实应用进程写入；E2E 用它验证 A→B→A 没有重新执行 workspace 命令。
    const gitTracePath = join(localFixtureRoot, "git-trace2.json");
    process.env.NARRANT_E2E_GIT_TRACE = gitTracePath;
    process.env.GIT_TRACE2_EVENT = gitTracePath;
    process.env.NARRANT_GITHUB_API_URL = "http://127.0.0.1:18472";
    process.env.NARRANT_E2E_PREPARED = "true";
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
  } catch (reason) {
    delete process.env.NARRANT_E2E_PREPARED;
    await stopGitHubMock();
    await removeFixture(true);
    throw reason;
  }
}

await prepareE2EEnvironment();

export const config = {
  runner: "local",
  logLevel: "error",
  specs: ["./e2e/**/*.e2e.ts"],
  maxInstances: 1,
  waitforTimeout: 15000,
  services: [["@wdio/tauri-service", {
    appBinaryPath: binary,
    driverProvider: "embedded",
  }]],
  capabilities: [{ browserName: "tauri", "tauri:options": { application: binary } }],
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 120000 },
  onComplete: async () => {
    await stopGitHubMock();
    // Windows 的 onComplete 早于 Tauri service 释放 SQLite handle，runner 退出后会回收临时目录。
    await removeFixture();
  }
};

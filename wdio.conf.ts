import { spawn, type ChildProcess } from "node:child_process";

const binary = `./src-tauri/target/debug/narrant${process.platform === "win32" ? ".exe" : ""}`;
let githubMock: ChildProcess | undefined;

export const config = {
  runner: "local",
  logLevel: "error",
  specs: ["./e2e/**/*.e2e.ts"],
  maxInstances: 1,
  services: [["@wdio/tauri-service", { appBinaryPath: binary, driverProvider: "embedded" }]],
  capabilities: [{ browserName: "tauri", "tauri:options": { application: binary } }],
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 120000 },
  onPrepare: async () => {
    process.env.NARRANT_GITHUB_API_URL = "http://127.0.0.1:18472";
    githubMock = spawn(process.execPath, ["e2e/fixtures/github-mock-server.mjs"], { stdio: "inherit" });
    for (let attempt = 0; attempt < 50; attempt += 1) {
      try { if ((await fetch("http://127.0.0.1:18472/health")).ok) return; } catch { /* retry */ }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error("GitHub mock server did not start");
  },
  onComplete: () => { githubMock?.kill(); }
};

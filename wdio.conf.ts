import type { Options } from "@wdio/types";

export const config: Options.Testrunner = {
  runner: "local",
  specs: ["./e2e/**/*.e2e.ts"],
  maxInstances: 1,
  capabilities: [{ browserName: "tauri" }],
  services: [["tauri", { application: "./src-tauri" }]],
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 120000 }
};

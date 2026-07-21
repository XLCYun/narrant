const binary = `./src-tauri/target/debug/narrant${process.platform === "win32" ? ".exe" : ""}`;

export const config = {
  runner: "local",
  specs: ["./e2e/**/*.e2e.ts"],
  maxInstances: 1,
  services: [["@wdio/tauri-service", { appBinaryPath: binary, driverProvider: "embedded" }]],
  capabilities: [{ browserName: "tauri", "tauri:options": { application: binary } }],
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 120000 }
};

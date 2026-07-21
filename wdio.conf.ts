export const config = {
  runner: "local",
  specs: ["./e2e/**/*.e2e.ts"],
  maxInstances: 1,
  services: [["@wdio/tauri-service", { appBinaryPath: "./src-tauri/target/debug/narrant", driverProvider: "embedded" }]],
  capabilities: [{ browserName: "tauri", "tauri:options": { application: "./src-tauri/target/debug/narrant" } }],
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 120000 }
};

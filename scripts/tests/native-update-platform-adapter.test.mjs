import assert from "node:assert/strict";
import test from "node:test";

import { normalizeWindowsInstallLocation, createNativeUpdatePlatformAdapter } from "../native-update-platform-adapter.mjs";

test("production installer smoke rejects local and self-hosted environments before touching data", async () => {
  for (const environment of [{}, { GITHUB_ACTIONS: "true", RUNNER_ENVIRONMENT: "self-hosted" }]) {
    const adapter = createNativeUpdatePlatformAdapter('darwin-aarch64', environment);
    await assert.rejects(adapter.initialize(), /GitHub-hosted/);
    await adapter.cleanup();
  }
});

test("Windows InstallLocation 接受绝对路径及 NSIS 外层双引号", () => {
  const location = String.raw`C:\Users\runneradmin\AppData\Local\Narrant`;
  assert.equal(normalizeWindowsInstallLocation(location), location);
  assert.equal(normalizeWindowsInstallLocation(`"${location}"`), location);
});

test("Windows InstallLocation 拒绝不配对、内嵌引号和非绝对路径", () => {
  for (const value of [
    "",
    String.raw`"C:\Narrant`,
    String.raw`C:\Narrant"`,
    String.raw`"C:\Nar"rant"`,
    String.raw`relative\Narrant`,
    String.raw` C:\Narrant`,
    "C:\\Narrant\nother",
    "C:\\Narrant\0other",
  ]) {
    assert.throws(() => normalizeWindowsInstallLocation(value), /InstallLocation/);
  }
});

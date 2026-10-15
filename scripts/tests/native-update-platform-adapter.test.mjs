import assert from "node:assert/strict";
import test from "node:test";
import { release, version } from "node:os";

import { describeUpdateSmokeRunner, normalizeWindowsInstallLocation } from "../native-update-platform-adapter.mjs";

test("自托管更新证据记录真实主机版本，缺少身份时不生成托管镜像名称", t => {
  const target = process.platform === "darwin" && process.arch === "arm64" ? "darwin-aarch64"
    : process.platform === "win32" && process.arch === "x64" ? "windows-x86_64" : undefined;
  if (!target) return t.skip("仅产品支持的 macOS ARM64/Windows x64 产生原生更新证据");
  const runner = describeUpdateSmokeRunner(target, { RUNNER_ENVIRONMENT: "self-hosted", RUNNER_NAME: "test-system" });
  assert.equal(runner.image, "self-hosted");
  assert.equal(runner.name, "test-system");
  assert.equal(runner.osRelease, release());
  assert.equal(runner.osVersion, version());
  assert.throws(() => describeUpdateSmokeRunner(target, {}), /自托管 runner 身份/);
  assert.throws(() => describeUpdateSmokeRunner("unsupported", {}), /target 不匹配/);
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

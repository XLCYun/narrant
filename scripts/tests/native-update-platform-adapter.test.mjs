import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { release, tmpdir, version } from "node:os";
import path from "node:path";

import { createNativeUpdatePlatformAdapter, describeUpdateSmokeRunner, normalizeWindowsInstallLocation, platformStatePaths } from "../native-update-platform-adapter.mjs";

// 直接经过初始化失败和真实清理入口，证明已有用户数据不会被失败收尾删除。
for (const existing of ["dataRoot", "cacheRoot"]) {
  test(`初始化拒绝已有 ${existing} 后，清理仍保留原数据`, async t => {
    const target = process.platform === "darwin" && process.arch === "arm64" ? "darwin-aarch64"
      : process.platform === "win32" && process.arch === "x64" ? "windows-x86_64" : undefined;
    if (!target) return t.skip("仅在产品支持的平台执行文件系统回归");
    const directory = await mkdtemp(path.join(tmpdir(), "narrant-cleanup-ownership-"));
    t.after(() => rm(directory, { recursive: true, force: true }));
    const environment = {
      RUNNER_ENVIRONMENT: "self-hosted", RUNNER_NAME: "fixture",
      HOME: directory, APPDATA: path.join(directory, "roaming"), LOCALAPPDATA: path.join(directory, "local"),
    };
    const roots = platformStatePaths(target, environment);
    await mkdir(roots[existing], { recursive: true });
    const sentinel = path.join(roots[existing], "personal-data.txt");
    await writeFile(sentinel, "keep my data");
    const adapter = createNativeUpdatePlatformAdapter(target, environment, {
      runCommand: async () => { throw new Error("拒绝已有数据时不得启动应用或系统命令"); },
    });
    await assert.rejects(adapter.initialize(), /旧 Narrant state/);
    await adapter.cleanup();
    assert.equal(await readFile(sentinel, "utf8"), "keep my data");
  });
}

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

test("日常桌面在任何系统操作前拒绝尚未隔离的正式更新验收", async t => {
  const target = process.platform === "darwin" && process.arch === "arm64" ? "darwin-aarch64"
    : process.platform === "win32" && process.arch === "x64" ? "windows-x86_64" : undefined;
  if (!target) return t.skip("仅产品支持的平台执行原生验收");
  let commandCount = 0;
  const adapter = createNativeUpdatePlatformAdapter(target, {
    RUNNER_ENVIRONMENT: "self-hosted", RUNNER_NAME: "fixture", NARRANT_CI_SHARED_DESKTOP: "1",
  }, { runCommand: async () => { commandCount++; throw new Error("不应执行系统命令"); } });
  await assert.rejects(adapter.initialize(), /日常桌面安全运行/);
  await adapter.cleanup();
  assert.equal(commandCount, 0);
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

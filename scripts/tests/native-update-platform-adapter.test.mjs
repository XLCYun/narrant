import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { release, tmpdir, version } from "node:os";
import path from "node:path";
import process from "node:process";
import { once } from "node:events";

import { createNativeUpdatePlatformAdapter, describeUpdateSmokeRunner, launchInstalled, normalizeWindowsInstallLocation, platformStatePaths, verifyPreservedUpgradeData } from "../native-update-platform-adapter.mjs";

test("启动后枚举失败仍交回准确 child 所有权，收尾不依赖枚举成功", async t => {
  let owned;
  let enumerations = 0;
  t.after(async () => {
    if (owned?.child.exitCode === null && owned.child.signalCode === null) {
      const exited = once(owned.child, "exit");
      owned.child.kill("SIGKILL");
      await exited;
    }
  });
  await assert.rejects(() => launchInstalled(process.platform === "win32" ? "windows-x86_64" : "darwin-aarch64",
    { executable: process.execPath }, async () => {
      if (++enumerations === 1) return "";
      throw new Error("enumeration failed");
    }, process.env, launch => { owned = launch; }), /enumeration failed/);
  assert.ok(owned?.pid > 0);
  assert.equal(owned.pid, owned.child.pid);
});

test("升级保留断言拒绝项目丢失、身份重建和设置重置", () => {
  const baseline = { theme: "dark", language: "en-US", navigation: {
    repositories: [{ id: 7, canonicalPath: "/fixture", availability: { kind: "available" } }], recentReviews: [],
  } };
  assert.doesNotThrow(() => verifyPreservedUpgradeData(structuredClone(baseline), baseline, "/fixture"));
  for (const mutate of [
    (value) => { value.navigation.repositories = []; },
    (value) => { value.navigation.repositories[0].id = 8; },
    (value) => { value.theme = "system"; },
    (value) => { value.language = "zh-CN"; },
  ]) {
    const changed = structuredClone(baseline);
    mutate(changed);
    assert.throws(() => verifyPreservedUpgradeData(changed, baseline, "/fixture"), /升级/);
  }
});

test("升级观察接受 Windows canonicalize 长路径，仍拒绝其他仓库和不同 POSIX 路径", () => {
  for (const [observed, expected] of [
    [String.raw`\\?\C:\Users\runner\repo`, String.raw`C:\Users\runner\repo`],
    [String.raw`\\?\UNC\server\share\repo`, String.raw`\\server\share\repo`],
  ]) {
    const value = { theme: "dark", language: "en-US", navigation: {
      repositories: [{ canonicalPath: observed, availability: { kind: "available" } }],
    } };
    assert.doesNotThrow(() => verifyPreservedUpgradeData(value, undefined, expected));
    assert.throws(() => verifyPreservedUpgradeData(value, undefined, `${expected}-other`), /升级/);
    value.navigation.repositories[0].canonicalPath = "/Fixture";
    assert.throws(() => verifyPreservedUpgradeData(value, undefined, "/fixture"), /升级/);
  }
});

// 直接经过初始化失败和真实清理入口，证明已有用户数据不会被失败收尾删除。
for (const existing of ["dataRoot", "cacheRoot"]) {
  test(`初始化拒绝已有 ${existing} 后，清理仍保留原数据`, async t => {
    const target = process.platform === "darwin" && process.arch === "arm64" ? "darwin-aarch64"
      : process.platform === "win32" && process.arch === "x64" ? "windows-x86_64" : undefined;
    if (!target) return t.skip("仅在产品支持的平台执行文件系统回归");
    const directory = await mkdtemp(path.join(tmpdir(), "narrant-cleanup-ownership-"));
    t.after(() => rm(directory, { recursive: true, force: true }));
    const environment = {
      RUNNER_ENVIRONMENT: "self-hosted", RUNNER_NAME: "fixture", NARRANT_UPGRADE_TEST_PROFILE: "1",
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

test("缺少隔离 profile 时在任何系统操作前拒绝更新验收", async t => {
  const target = process.platform === "darwin" && process.arch === "arm64" ? "darwin-aarch64"
    : process.platform === "win32" && process.arch === "x64" ? "windows-x86_64" : undefined;
  if (!target) return t.skip("仅产品支持的平台执行原生验收");
  let commandCount = 0;
  const adapter = createNativeUpdatePlatformAdapter(target, {
    RUNNER_ENVIRONMENT: "self-hosted", RUNNER_NAME: "fixture", NARRANT_CI_SHARED_DESKTOP: "1",
  }, { runCommand: async () => { commandCount++; throw new Error("不应执行系统命令"); } });
  await assert.rejects(adapter.initialize(), /独立测试包 profile/);
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

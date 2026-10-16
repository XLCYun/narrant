import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  inspectDurableStaging,
  platformStatePaths,
} from "../native-update-platform-adapter.mjs";
import { combineDriverErrors, runNativeUpdateSmokeDriver } from "../native-update-smoke-driver.mjs";
import { NORMAL_CHECKPOINTS } from "../update-smoke-evidence.mjs";

/** 创建可记录方法顺序的 fake native adapter。 */
function adapter(events, fail = {}) {
  const invoke = async (name, value) => {
    events.push(name);
    if (fail[name]) throw new Error(fail[name]);
    return value;
  };
  return {
    runner: { os: "macos", arch: "aarch64", image: "macos-15" },
    platform: { packageKind: "app-tar-gz", installerKind: "dmg", installScope: "user", codeSigning: "verified", notarization: "verified" },
    initialize: () => invoke("initialize"),
    installBaseline: () => invoke("installBaseline", { root: "/app" }),
    withCandidateGate: async (_options, action) => {
      events.push("gate:start");
      const result = await action({ waitForAllRequests: () => invoke("gate:requests") });
      events.push("gate:end");
      return result;
    },
    launch: () => invoke("launch", { pid: 7 }),
    waitForStaging: () => invoke("staging"),
    requestNormalClose: () => invoke("close"),
    waitForExit: () => invoke("exit"),
    inspectUpdated: () => invoke("updated"),
    assertNotRelaunched: () => invoke("no-relaunch"),
    inspectTrustedConfiguration: () => invoke("trust"),
    inspectPreservedData: () => invoke("preserved-data"),
    cleanup: () => invoke("cleanup"),
  };
}

/** driver 固定 options。 */
function options(mode = "normal") {
  return {
    target: "darwin-aarch64", mode, version: "2.0.0", publicTag: "v2.0.0",
    candidateDirectory: "/candidate", candidateInstaller: "/installer",
    releaseSigningPublicKey: "old-public", candidateUpdaterPublicKey: "successor-public",
    previousReleaseSigningPublicKey: mode === "normal" ? "previous-old-public" : undefined,
    manifest: { version: "2.0.0" },
    previous: mode === "normal" ? { tag: "v1.9.0", version: "1.9.0" } : null,
    baselinePackage: mode === "normal" ? "/previous" : undefined,
    baselineSignature: mode === "normal" ? "/previous.sig" : undefined,
  };
}

test("normal driver 按 gate/staging/normal-exit/no-relaunch/manual-launch 顺序观察", async () => {
  const events = [];
  const result = await runNativeUpdateSmokeDriver(options(), adapter(events));
  assert.deepEqual(events, [
    "initialize", "installBaseline", "gate:start", "launch", "staging", "gate:requests",
    "close", "exit", "updated", "no-relaunch", "launch", "trust", "preserved-data", "close", "exit", "gate:end", "cleanup",
  ]);
  assert.deepEqual(result.checkpoints.map(({ id }) => id), [...NORMAL_CHECKPOINTS]);
});

test("拒绝已完成使命的 bootstrap，避免误安装正式候选", async () => {
  await assert.rejects(() => runNativeUpdateSmokeDriver(options("bootstrap"), adapter([])), /只支持 normal/);
});

test("normal 必须提供 previous release key，bootstrap 必须拒绝该 key", async () => {
  const missing = options();
  delete missing.previousReleaseSigningPublicKey;
  await assert.rejects(() => runNativeUpdateSmokeDriver(missing, adapter([])), /previous.*key/i);
  const bootstrap = options("bootstrap");
  bootstrap.previousReleaseSigningPublicKey = "unexpected-old-key";
  await assert.rejects(() => runNativeUpdateSmokeDriver(bootstrap, adapter([])), /只支持 normal/);
});

test("cleanup 错误不覆盖 native 主错误", async () => {
  const events = [];
  await assert.rejects(() => runNativeUpdateSmokeDriver(options(), adapter(events, { staging: "staging failed", cleanup: "cleanup failed" })), (error) => {
    assert.equal(error instanceof AggregateError, true);
    assert.equal(error.message, "staging failed");
    return true;
  });
  assert.equal(events.at(-1), "cleanup");
  assert.equal(combineDriverErrors(new Error("primary"), new Error("cleanup")).message, "primary");
});

test("durable staging 必须匹配 pointer/metadata/package digest", async (context) => {
  const root = await mkdtemp(join(tmpdir(), "narrant-staging-test-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  const generation = "generation-00000000-0000-4000-8000-000000000001";
  await mkdir(join(root, generation));
  const packageBytes = Buffer.from("candidate-package");
  const expected = {
    version: "2.0.0", signature: "literal-signature", target: "darwin-aarch64",
    installKind: "macos_app_archive",
    packageSha256: "aaa7d99be9b89470c00e6883d517fce652354dd4e67e20abe98ef78a5256d0ed",
  };
  await Promise.all([
    writeFile(join(root, "current"), generation),
    writeFile(join(root, generation, "package.bin"), packageBytes),
    writeFile(join(root, generation, "metadata.json"), JSON.stringify({ schemaVersion: 2, version: "2.0.0", signature: "literal-signature", target: "darwin-aarch64", installKind: "macos_app_archive", releaseNotes: "notes" })),
  ]);
  assert.equal((await inspectDurableStaging(root, expected)).generation, generation);
  await writeFile(join(root, generation, "package.bin"), "tampered");
  await assert.rejects(() => inspectDurableStaging(root, expected), /identity/);
});

test("平台 state path 固定在 Tauri identifier 的 data/cache 边界", () => {
  const mac = platformStatePaths("darwin-aarch64", { HOME: "/Users/runner" });
  assert.equal(mac.stagingRoot, "/Users/runner/Library/Caches/com.cyunlab.narrant.upgrade-test/application-update");
  const windows = platformStatePaths("windows-x86_64", { APPDATA: "C:\\Users\\runner\\AppData\\Roaming", LOCALAPPDATA: "C:\\Users\\runner\\AppData\\Local" });
  assert.equal(windows.stagingRoot, "C:\\Users\\runner\\AppData\\Local\\com.cyunlab.narrant.upgrade-test\\application-update");
});

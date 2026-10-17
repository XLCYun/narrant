import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  inspectDurableStaging,
  launchInstalled,
  observeBootstrapRecovery,
  platformStatePaths,
  writeBootstrapVerificationFixture,
} from "../native-update-platform-adapter.mjs";
import { combineDriverErrors, runNativeUpdateSmokeDriver } from "../native-update-smoke-driver.mjs";
import { BOOTSTRAP_CHECKPOINTS, NORMAL_CHECKPOINTS } from "../update-smoke-evidence.mjs";
import { driverEnvironment } from "../run-native-update-smoke.mjs";

test("driver 保留 hosted 保护所需标记，但不继承发布凭据", () => {
  assert.deepEqual(driverEnvironment({
    GITHUB_ACTIONS: "true", RUNNER_ENVIRONMENT: "github-hosted", RUNNER_TEMP: "/runner/tmp",
    GH_TOKEN: "must-not-leak", TAURI_SIGNING_PRIVATE_KEY: "must-not-leak",
  }), { GITHUB_ACTIONS: "true", RUNNER_ENVIRONMENT: "github-hosted", RUNNER_TEMP: "/runner/tmp" });
});

test("发现 PID 后仍等待原生窗口就绪，启动探测失败不能通过验收", async () => {
  const installation = { application: "/fixture/Narrant.app", executable: "/fixture/Narrant.app/Contents/MacOS/narrant" };
  let opened = false;
  let probes = 0;
  /** 模拟系统发现进程早于应用窗口就绪，不启动真实 Narrant。 */
  const command = async (executable, arguments_) => {
    if (executable === "open") { opened = true; return ""; }
    if (executable === "ps") return opened ? `42 ${installation.executable}\n` : "";
    assert.equal(executable, "/fixture/window-helper");
    assert.deepEqual(arguments_, ["42", "ready"]);
    probes += 1;
    return probes === 1 ? "" : "ready\n";
  };
  const launch = await launchInstalled("darwin-aarch64", installation, command, {}, "/fixture/window-helper");
  assert.equal(launch.pid, 42);
  assert.equal(probes, 2);

  opened = false;
  await assert.rejects(() => launchInstalled("darwin-aarch64", installation, async (executable, arguments_) => {
    if (executable === "/fixture/window-helper") throw new Error("window startup failed");
    return command(executable, arguments_);
  }, {}, "/fixture/window-helper"), /window startup failed/);
});

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
    installBootstrap: () => invoke("installBootstrap", { root: "/app" }),
    prepareUserData: () => invoke("prepare-data"),
    registerUserRepository: () => invoke("register-project"),
    assertUserData: () => invoke("check-data"),
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
    proveBootstrapRecovery: () => invoke("recovery", { pid: 8 }),
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
    "initialize", "installBaseline", "prepare-data", "gate:start", "launch", "staging", "gate:requests", "register-project", "check-data",
    "close", "exit", "updated", "no-relaunch", "launch", "trust", "close", "exit", "check-data", "gate:end", "cleanup",
  ]);
  assert.deepEqual(result.checkpoints.map(({ id }) => id), [...NORMAL_CHECKPOINTS]);
});

test("bootstrap driver 不调用 baseline/gate，真实恢复后正常退出并 clean relaunch", async () => {
  const events = [];
  const result = await runNativeUpdateSmokeDriver(options("bootstrap"), adapter(events));
  assert.deepEqual(events, ["initialize", "installBootstrap", "recovery", "trust", "close", "exit", "launch", "close", "exit", "cleanup"]);
  assert.deepEqual(result.checkpoints.map(({ id }) => id), [...BOOTSTRAP_CHECKPOINTS]);
});

test("normal 必须提供 previous release key，bootstrap 必须拒绝该 key", async () => {
  const missing = options();
  delete missing.previousReleaseSigningPublicKey;
  await assert.rejects(() => runNativeUpdateSmokeDriver(missing, adapter([])), /previous.*key/i);
  const bootstrap = options("bootstrap");
  bootstrap.previousReleaseSigningPublicKey = "unexpected-old-key";
  await assert.rejects(() => runNativeUpdateSmokeDriver(bootstrap, adapter([])), /bootstrap.*previous.*key/i);
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
  assert.equal(mac.stagingRoot, "/Users/runner/Library/Caches/com.cyunlab.narrant/application-update");
  const windows = platformStatePaths("windows-x86_64", { APPDATA: "C:\\Users\\runner\\AppData\\Roaming", LOCALAPPDATA: "C:\\Users\\runner\\AppData\\Local" });
  assert.equal(windows.stagingRoot, "C:\\Users\\runner\\AppData\\Local\\com.cyunlab.narrant\\application-update");
});

test("bootstrap fixture 以严格更高版本进入验签分支并观察完整 recovery cleanup/diagnostic", async (context) => {
  const root = await mkdtemp(join(tmpdir(), "narrant-bootstrap-recovery-"));
  context.after(() => rm(root, { recursive: true, force: true }));
  const state = {
    stagingRoot: join(root, "cache", "application-update"),
    diagnostics: join(root, "data", "logs", "application-update.jsonl"),
  };
  const fixture = await writeBootstrapVerificationFixture(state.stagingRoot, {
    currentVersion: "2.0.9",
    target: "darwin-aarch64",
    installKind: "macos_app_archive",
  });
  assert.equal(fixture.version, "2.0.10");
  const metadata = JSON.parse(await readFile(join(state.stagingRoot, fixture.generation, "metadata.json"), "utf8"));
  assert.deepEqual(metadata, {
    schemaVersion: 2,
    version: "2.0.10",
    signature: "invalid-bootstrap-verification-signature",
    target: "darwin-aarch64",
    installKind: "macos_app_archive",
    releaseNotes: "bootstrap verification recovery fixture",
  });

  await rm(join(state.stagingRoot, "current"));
  await mkdir(join(root, "data", "logs"), { recursive: true });
  await writeFile(state.diagnostics, '{"event":"transition-warning","sequence":1,"version":null,"category":"verification","operationId":null,"httpStatus":null}\n');
  await assert.rejects(() => observeBootstrapRecovery(state, fixture.generation, { timeoutMilliseconds: 20 }), /generation/);
  await rm(join(state.stagingRoot, fixture.generation), { recursive: true });
  await assert.doesNotReject(() => observeBootstrapRecovery(state, fixture.generation, { timeoutMilliseconds: 100 }));
});

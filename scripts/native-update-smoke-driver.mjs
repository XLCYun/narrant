import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import process from "node:process";

import { createNativeUpdatePlatformAdapter } from "./native-update-platform-adapter.mjs";
import { BOOTSTRAP_CHECKPOINTS, NORMAL_CHECKPOINTS } from "./update-smoke-evidence.mjs";

/** 合并主流程与 cleanup 错误，同时保持主错误消息。 */
export function combineDriverErrors(primary, cleanup) {
  if (!primary) return cleanup;
  if (!cleanup) return primary;
  return new AggregateError([primary, cleanup], primary.message);
}

/** 校验 driver 只接受两个 production target 和显式 normal/bootstrap。 */
function validateOptions(options) {
  if (!["darwin-aarch64", "windows-x86_64"].includes(options.target)) throw new Error("native driver target 无效");
  if (!["normal", "bootstrap"].includes(options.mode)) throw new Error("native driver mode 无效");
  for (const name of ["version", "publicTag", "candidateDirectory", "candidateInstaller", "publicKey", "manifest"]) {
    if (!options[name]) throw new Error(`native driver 缺少 ${name}`);
  }
  if (options.mode === "normal" && (!options.previous?.tag || !options.previous?.version || !options.baselinePackage || !options.baselineSignature)) {
    throw new Error("normal native driver 缺少 previous Stable inputs");
  }
  if (options.mode === "bootstrap" && options.previous !== null) throw new Error("bootstrap 不得提供 previous Stable");
}

/** 运行 previous Stable→candidate 的真实 normal-exit 更新。 */
async function runNormal(options, adapter) {
  const baseline = await adapter.installBaseline(options);
  await adapter.withCandidateGate(options, async (gate) => {
    const baselineLaunch = await adapter.launch();
    await Promise.all([adapter.waitForStaging(options), gate.waitForAllRequests()]);
    await adapter.requestNormalClose(baselineLaunch);
    await adapter.waitForExit(baselineLaunch);
    await adapter.inspectUpdated(options, baseline);
    await adapter.assertNotRelaunched();
    await adapter.launch();
    await adapter.inspectTrustedConfiguration(options.publicKey);
  });
  return NORMAL_CHECKPOINTS;
}

/** 运行一次性 first-updater signed install/config/staging-recovery bootstrap。 */
async function runBootstrap(options, adapter) {
  await adapter.installBootstrap(options);
  const recoveryLaunch = await adapter.proveBootstrapRecovery(options);
  await adapter.inspectTrustedConfiguration(options.publicKey);
  await adapter.requestNormalClose(recoveryLaunch);
  await adapter.waitForExit(recoveryLaunch);
  await adapter.launch();
  return BOOTSTRAP_CHECKPOINTS;
}

/**
 * 执行 repository-owned native driver；checkpoints 只在各真实系统调用完成后一次性生成，
 * 生产调用不能注入 fixture adapter。
 */
export async function runNativeUpdateSmokeDriver(options, adapter) {
  validateOptions(options);
  const startedAt = new Date().toISOString();
  let primary;
  let result;
  try {
    await adapter.initialize();
    const checkpoints = options.mode === "normal"
      ? await runNormal(options, adapter)
      : await runBootstrap(options, adapter);
    result = Object.freeze({
      runner: adapter.runner,
      platform: adapter.platform,
      startedAt,
      completedAt: new Date().toISOString(),
      checkpoints: checkpoints.map((id) => Object.freeze({ id, status: "passed" })),
    });
  } catch (error) {
    primary = error;
  }
  let cleanup;
  try { await adapter.cleanup(); } catch (error) { cleanup = error; }
  const failure = combineDriverErrors(primary, cleanup);
  if (failure) throw failure;
  if (!result) throw new Error("native driver 未产生 observation");
  return result;
}

/** 解析固定成对 CLI arguments。 */
function parseArguments(arguments_) {
  const allowed = new Set([
    "target", "mode", "version", "public-tag", "candidate-directory", "candidate-installer",
    "candidate-manifest", "previous-json", "baseline-package", "baseline-signature",
  ]);
  const values = {};
  for (let index = 0; index < arguments_.length; index += 2) {
    const name = arguments_[index];
    const value = arguments_[index + 1];
    const key = name?.slice(2);
    if (!name?.startsWith("--") || value === undefined || !allowed.has(key) || Object.hasOwn(values, key)) {
      throw new Error(`native driver 参数无效：${name ?? ""}`);
    }
    values[key] = value;
  }
  return values;
}

/** production CLI 只向 stdout 输出一份有界 JSON observation。 */
async function main() {
  const values = parseArguments(process.argv.slice(2));
  const publicKey = process.env.NARRANT_UPDATER_PUBLIC_KEY;
  if (!publicKey) throw new Error("native driver 缺少 NARRANT_UPDATER_PUBLIC_KEY");
  const options = {
    target: values.target,
    mode: values.mode,
    version: values.version,
    publicTag: values["public-tag"],
    candidateDirectory: values["candidate-directory"],
    candidateInstaller: values["candidate-installer"],
    publicKey,
    manifest: JSON.parse(await readFile(values["candidate-manifest"], "utf8")),
    previous: values["previous-json"] ? JSON.parse(await readFile(values["previous-json"], "utf8")) : null,
    baselinePackage: values["baseline-package"],
    baselineSignature: values["baseline-signature"],
  };
  const result = await runNativeUpdateSmokeDriver(options, createNativeUpdatePlatformAdapter(options.target));
  const body = Buffer.from(JSON.stringify(result));
  if (body.length > 256 * 1024) throw new Error("native observation 超过上限");
  process.stdout.write(body);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : "native driver 失败"}\n`);
    process.exitCode = 1;
  });
}
import { Buffer } from "node:buffer";

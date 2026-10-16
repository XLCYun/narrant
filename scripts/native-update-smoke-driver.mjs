import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import process from "node:process";

import { createNativeUpdatePlatformAdapter } from "./native-update-platform-adapter.mjs";
import { NORMAL_CHECKPOINTS } from "./update-smoke-evidence.mjs";

/** 合并主流程与 cleanup 错误，同时保持主错误消息。 */
export function combineDriverErrors(primary, cleanup) {
  if (!primary) return cleanup;
  if (!cleanup) return primary;
  return new AggregateError([primary, cleanup], primary.message);
}

/** 校验 driver 只接受两个支持平台的隔离升级。 */
function validateOptions(options) {
  if (!["darwin-aarch64", "windows-x86_64"].includes(options.target)) throw new Error("native driver target 无效");
  if (options.mode !== "normal") throw new Error("隔离升级测试只支持 normal 模式；首次 updater bootstrap 已完成");
  for (const name of [
    "version", "publicTag", "candidateDirectory", "candidateInstaller",
    "releaseSigningPublicKey", "candidateUpdaterPublicKey", "manifest",
  ]) {
    if (!options[name]) throw new Error(`native driver 缺少 ${name}`);
  }
  if (!options.previousReleaseSigningPublicKey) {
    throw new Error("normal native driver 缺少 previous release key");
  }
  if (
    !options.previous?.tag || !options.previous?.version || !options.baselinePackage || !options.baselineSignature
  ) {
    throw new Error("normal native driver 缺少 previous Stable inputs");
  }
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
    const candidateLaunch = await adapter.launch("candidate");
    await adapter.inspectTrustedConfiguration(options.candidateUpdaterPublicKey);
    await adapter.inspectPreservedData(options);
    await adapter.requestNormalClose(candidateLaunch);
    await adapter.waitForExit(candidateLaunch);
  });
  return NORMAL_CHECKPOINTS;
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
    const checkpoints = await runNormal(options, adapter);
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
  const releaseSigningPublicKey = process.env.NARRANT_RELEASE_SIGNING_PUBLIC_KEY;
  const candidateUpdaterPublicKey = process.env.NARRANT_UPDATER_PUBLIC_KEY;
  const previousReleaseSigningPublicKey =
    process.env.NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY || undefined;
  if (!releaseSigningPublicKey || !candidateUpdaterPublicKey) {
    throw new Error("native driver 缺少 release signing/candidate updater public key");
  }
  const options = {
    target: values.target,
    mode: values.mode,
    version: values.version,
    publicTag: values["public-tag"],
    candidateDirectory: values["candidate-directory"],
    candidateInstaller: values["candidate-installer"],
    releaseSigningPublicKey,
    candidateUpdaterPublicKey,
    previousReleaseSigningPublicKey,
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

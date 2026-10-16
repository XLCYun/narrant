import { readFile } from "node:fs/promises";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { verifyUpdateSmokeEvidenceDocument, writeUpdateSmokeEvidence } from "./update-smoke-evidence.mjs";
import { resolvePreviousReleaseSigningKeySha256, resolveUpdaterKeyPolicy } from "./release-assets.mjs";
import { verifyUpdateTestBuilds, runIsolatedUpdateTest } from "./run-isolated-update-test.mjs";
import { verifyProductionUpdateArtifacts } from "./verify-production-update-artifacts.mjs";

/** 调用真实 driver 并写出与固定 Draft/源码身份绑定的 evidence。 */
export async function runNativeUpdateSmoke(options, environment = process.env) {
  const draftReceipt = JSON.parse(await readFile(options.draftReceipt, "utf8"));
  const updaterTrust = resolveUpdaterKeyPolicy({
    tag: options.publicTag,
    mode: options.mode,
    releaseSigningPublicKey: environment.NARRANT_RELEASE_SIGNING_PUBLIC_KEY,
    candidateUpdaterPublicKey: environment.NARRANT_UPDATER_PUBLIC_KEY,
    bridgeTag: environment.NARRANT_UPDATER_BRIDGE_TAG,
  });
  if (JSON.stringify(updaterTrust) !== JSON.stringify(draftReceipt.updaterTrust)) {
    throw new Error("native runner updater key identity 与已验证 Draft receipt 不匹配");
  }
  const previousReleaseSigningKeySha256 = resolvePreviousReleaseSigningKeySha256(
    options.mode,
    environment.NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY,
  );
  const previous = options.previousJson ? JSON.parse(await readFile(options.previousJson, "utf8")) : null;
  if (options.mode === "normal" &&
      previous?.previousReleaseSigningKeySha256 !== previousReleaseSigningKeySha256) {
    throw new Error("previous Stable key identity 与当前 normal smoke 输入不匹配");
  }
  if (options.mode !== "normal") throw new Error("首次 updater bootstrap 已完成；当前发布采用隔离 normal 升级");
  const { receipt: testBuilds } = await verifyUpdateTestBuilds(options.testBuildReceipt);
  if (testBuilds.target !== options.target || testBuilds.builds.candidate.sourceSha !== options.sourceSha ||
      testBuilds.builds.candidate.version !== options.version || testBuilds.builds.candidate.sourceVersion !== options.version ||
      testBuilds.builds.baseline.version !== previous?.version || testBuilds.builds.baseline.sourceSha !== options.previousSourceSha) {
    throw new Error("测试包源码或版本与正式发布不一致");
  }
  const productionChecks = await verifyProductionUpdateArtifacts({ ...options, draftReceipt, previous }, environment);
  const observation = await runIsolatedUpdateTest(options.testBuildReceipt, options.testOutputDirectory, environment);
  const evidence = {
    schemaVersion: 2,
    evidenceKind: "isolated-native",
    mode: options.mode,
    target: options.target,
    runner: observation.runner,
    platform: observation.platform,
    source: { repository: "XLCYun/narrant", tag: options.sourceTag, sha: options.sourceSha },
    candidate: {
      version: options.version,
      publicReleaseId: Number(options.publicReleaseId),
      publicTag: options.publicTag,
      manifestSha256: draftReceipt.manifestSha256,
    },
    updaterTrust,
    testBuilds: observation.testBuilds,
    productionChecks,
    draftReceipt,
    previousStable: previous ? {
      tag: previous.tag,
      version: previous.version,
      manifestSha256: previous.manifestSha256,
      packageSha256: previous.packageSha256,
      signatureSha256: previous.signatureSha256,
      previousReleaseSigningKeySha256,
      packageUrl: previous.packageUrl,
    } : null,
    startedAt: observation.startedAt,
    completedAt: observation.completedAt,
    checkpoints: observation.checkpoints,
  };
  verifyUpdateSmokeEvidenceDocument(evidence, {
    mode: options.mode,
    sourceTag: options.sourceTag,
    sourceSha: options.sourceSha,
    publicReleaseId: Number(options.publicReleaseId),
    publicTag: options.publicTag,
    version: options.version,
    manifestSha256: draftReceipt.manifestSha256,
    draftReceipt,
    previousTag: previous?.tag,
    previousManifestSha256: previous?.manifestSha256,
    previousReleaseSigningKeySha256,
    allowBootstrap: options.mode === "bootstrap",
    bootstrapTag: options.mode === "bootstrap" ? options.sourceTag : undefined,
    maxAgeHours: 24,
  });
  return { evidence, ...await writeUpdateSmokeEvidence(evidence, options.outputDirectory) };
}

/** 解析 workflow CLI 固定成对参数。 */
function parseArguments(arguments_) {
  const values = {};
  for (let index = 0; index < arguments_.length; index += 2) {
    const name = arguments_[index];
    const value = arguments_[index + 1];
    if (!name?.startsWith("--") || value === undefined) throw new Error(`无效参数：${name ?? ""}`);
    values[name.slice(2).replace(/-([a-z])/g, (_match, letter) => letter.toUpperCase())] = value;
  }
  return values;
}

/** 解析 workflow 参数并运行当前 hosted target 的真实原生升级证据流程。 */
async function main() {
  const options = parseArguments(process.argv.slice(2));
  const result = await runNativeUpdateSmoke(options);
  process.stdout.write(`已记录 ${result.evidence.mode} ${result.evidence.target} isolated-native evidence（正式包仅核验签名与完整性）。\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`错误：${error instanceof Error ? error.message : "native smoke 失败"}\n`);
    process.exitCode = 1;
  });
}

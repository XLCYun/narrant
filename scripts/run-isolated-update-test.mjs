import { createHash } from "node:crypto";
import { cp, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

import { UPDATE_TEST_PROFILE } from "./update-test-profile.mjs";
import { verifyTauriUpdaterSignature, updaterPublicKeySha256 } from "./release-assets.mjs";
import { createNativeUpdatePlatformAdapter } from "./native-update-platform-adapter.mjs";
import { runNativeUpdateSmokeDriver } from "./native-update-smoke-driver.mjs";

/** 计算被实际读取、签名检查和安装的文件摘要。 */
function sha256(bytes) { return createHash("sha256").update(bytes).digest("hex"); }

/** 核对本地构建记录与真实包，不把生产包误送入安装测试。 */
export async function verifyUpdateTestBuilds(receiptPath) {
  const receipt = JSON.parse(await readFile(receiptPath, "utf8"));
  if (receipt.schemaVersion !== 1 || !["darwin-aarch64", "windows-x86_64"].includes(receipt.target) || JSON.stringify(receipt.profile) !== JSON.stringify(UPDATE_TEST_PROFILE) ||
      !/^[0-9a-f]{64}$/.test(receipt.recipeSha256)) throw new Error("独立升级测试构建身份无效");
  const builds = {};
  for (const stage of ["baseline", "candidate"]) {
    const build = receipt.builds?.[stage];
    if (!build || !/^[0-9a-f]{40}$/.test(build.sourceSha) || !/^[0-9a-f]{64}$/.test(build.overlaySha256) ||
        !/^\d+\.\d+\.\d+$/.test(build.version) || path.basename(build.package) !== build.package || build.signature !== `${build.package}.sig`) throw new Error("升级测试源码或文件记录无效");
    const packagePath = path.resolve(path.dirname(receiptPath), build.package);
    const signaturePath = `${packagePath}.sig`;
    const bytes = await readFile(packagePath);
    const signature = await readFile(signaturePath, "utf8");
    const overlay = await readFile(path.resolve(path.dirname(receiptPath), `${stage}-overlay.patch`));
    if (sha256(bytes) !== build.packageSha256 || sha256(signature) !== build.signatureSha256 || sha256(overlay) !== build.overlaySha256 || overlay.toString("utf8") !== build.overlay) throw new Error("升级测试包或源码叠加已改变");
    verifyTauriUpdaterSignature(bytes, signature, receipt.publicKey);
    builds[stage] = { ...build, packagePath, signaturePath, signatureText: signature };
  }
  const previous = builds.baseline.version.split(".").map(Number);
  const candidate = builds.candidate.version.split(".").map(Number);
  const differing = candidate.findIndex((value, index) => value !== previous[index]);
  if (differing < 0 || candidate[differing] < previous[differing]) throw new Error("测试候选必须高于旧版");
  return { receipt, builds };
}

/** 真实运行隔离包；返回独立测试证据，单独运行不会获得正式版本的发布资格。 */
export async function runIsolatedUpdateTest(receiptPath, outputDirectory, environment = process.env) {
  const { receipt, builds } = await verifyUpdateTestBuilds(receiptPath);
  const output = path.resolve(outputDirectory);
  await mkdir(output);
  const candidateDirectory = path.join(output, "candidate-assets");
  await mkdir(candidateDirectory);
  const target = receipt.target;
  const packageName = target === "darwin-aarch64" ? "narrant-darwin-aarch64-updater.app.tar.gz" : "narrant-windows-x86_64-updater.exe";
  await cp(builds.candidate.packagePath, path.join(candidateDirectory, packageName));
  await cp(builds.candidate.signaturePath, path.join(candidateDirectory, `${packageName}.sig`));
  const manifest = {
    version: builds.candidate.version,
    notes: "Isolated upgrade test; not a production release",
    platforms: { [target]: {
      url: `https://updates.cyunlab.com/narrant/releases/${builds.candidate.version}/${target}/${builds.candidate.packageSha256}-${packageName}`,
      signature: builds.candidate.signatureText,
    } },
  };
  const manifestBytes = `${JSON.stringify(manifest)}\n`;
  await writeFile(path.join(candidateDirectory, "latest.json"), manifestBytes);
  const options = {
    target, mode: "normal", version: builds.candidate.version, publicTag: `v${builds.candidate.version}`,
    candidateDirectory, candidateInstaller: builds.candidate.packagePath, manifest,
    releaseSigningPublicKey: receipt.publicKey, candidateUpdaterPublicKey: receipt.publicKey, previousReleaseSigningPublicKey: receipt.publicKey,
    previous: { version: builds.baseline.version, tag: `v${builds.baseline.version}` },
    baselinePackage: builds.baseline.packagePath, baselineSignature: builds.baseline.signaturePath,
  };
  const observation = await runNativeUpdateSmokeDriver(options, createNativeUpdatePlatformAdapter(target, {
    ...environment, NARRANT_UPGRADE_TEST_PROFILE: "1",
  }));
  const result = {
    evidenceKind: "isolated-native-preflight",
    coverage: receipt.builds.baseline.sourceSha === receipt.builds.candidate.sourceSha
      ? "same-source-version-regression" : "distinct-source-upgrade",
    execution: environment.GITHUB_ACTIONS === "true" ? "github-actions" : "local",
    productionArtifactsVerified: false,
    ...observation,
    testBuilds: {
      ...receipt, publicKey: undefined, updaterKeySha256: updaterPublicKeySha256(receipt.publicKey),
      manifestSha256: sha256(manifestBytes),
    },
  };
  await writeFile(path.join(output, "isolated-test.json"), `${JSON.stringify(result, null, 2)}\n`);
  return result;
}

/** CLI 默认记录预合并验收，不伪造正式 Draft 或发布门禁结果。 */
async function main() {
  const [receipt, output] = process.argv.slice(2);
  if (!receipt || !output) throw new Error("需要测试构建记录和独立证据目录");
  await runIsolatedUpdateTest(path.resolve(receipt), output);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => { process.stderr.write(`${error.message}\n`); process.exitCode = 1; });
}

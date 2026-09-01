import { spawn } from "node:child_process";
import { Buffer } from "node:buffer";
import { readFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

import { verifyUpdateSmokeEvidenceDocument, writeUpdateSmokeEvidence } from "./update-smoke-evidence.mjs";

const DRIVER = path.resolve(import.meta.dirname, "native-update-smoke-driver.mjs");
const OUTPUT_BOUND = 256 * 1024;

/** 只向 native driver 传公共 runner 环境和 updater public key。 */
function driverEnvironment(environment) {
  const names = [
    "PATH", "SystemRoot", "SystemDrive", "windir", "ComSpec", "PATHEXT", "PSModulePath", "OS",
    "HOME", "USERPROFILE", "USERNAME", "HOMEDRIVE", "HOMEPATH", "LOCALAPPDATA", "APPDATA",
    "ProgramData", "ProgramFiles", "ProgramFiles(x86)", "TEMP", "TMP", "TMPDIR", "LANG", "LC_ALL",
    "NARRANT_UPDATER_PUBLIC_KEY",
  ];
  return Object.fromEntries(names.flatMap((name) => environment[name] === undefined ? [] : [[name, environment[name]]]));
}

/** 无 shell 启动固定 repository driver，并限制输出。 */
function runDriver(arguments_, environment) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [DRIVER, ...arguments_], {
      env: driverEnvironment(environment), shell: false, windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stdout = [];
    let size = 0;
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      size += chunk.length;
      if (size > OUTPUT_BOUND) child.kill(); else stdout.push(chunk);
    });
    child.stderr.on("data", (chunk) => { if (Buffer.byteLength(stderr) < OUTPUT_BOUND) stderr += chunk.toString("utf8"); });
    child.once("error", reject);
    child.once("close", (code) => {
      if (size > OUTPUT_BOUND) reject(new Error("native driver 输出超过上限"));
      else if (code !== 0) reject(new Error(`native driver 失败（${code}）：${stderr.trim().slice(0, 2048)}`));
      else resolve(Buffer.concat(stdout));
    });
  });
}

/** 调用真实 driver 并写出与固定 Draft/源码身份绑定的 evidence。 */
export async function runNativeUpdateSmoke(options, environment = process.env, dependencies = {}) {
  const draftReceipt = JSON.parse(await readFile(options.draftReceipt, "utf8"));
  const previous = options.previousJson ? JSON.parse(await readFile(options.previousJson, "utf8")) : null;
  const arguments_ = [
    "--target", options.target,
    "--mode", options.mode,
    "--version", options.version,
    "--public-tag", options.publicTag,
    "--candidate-directory", options.candidateDirectory,
    "--candidate-installer", options.candidateInstaller,
    "--candidate-manifest", path.join(options.candidateDirectory, "latest.json"),
  ];
  if (options.mode === "normal") arguments_.push(
    "--previous-json", options.previousJson,
    "--baseline-package", previous.packagePath,
    "--baseline-signature", previous.signaturePath,
  );
  const output = dependencies.runDriver
    ? await dependencies.runDriver(arguments_, environment)
    : await runDriver(arguments_, environment);
  let observation;
  try { observation = JSON.parse(Buffer.from(output).toString("utf8")); }
  catch { throw new Error("native driver 未返回有效 JSON observation"); }
  const evidence = {
    schemaVersion: 1,
    evidenceKind: "real-native",
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
    draftReceipt,
    previousStable: previous ? {
      tag: previous.tag,
      version: previous.version,
      manifestSha256: previous.manifestSha256,
      packageSha256: previous.packageSha256,
      signatureSha256: previous.signatureSha256,
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

async function main() {
  const options = parseArguments(process.argv.slice(2));
  const result = await runNativeUpdateSmoke(options);
  process.stdout.write(`已记录 ${result.evidence.mode} ${result.evidence.target} real-native evidence。\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`错误：${error instanceof Error ? error.message : "native smoke 失败"}\n`);
    process.exitCode = 1;
  });
}

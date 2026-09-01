import { createHash } from "node:crypto";
import { Buffer } from "node:buffer";
import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { join } from "node:path";
import process from "node:process";
import { pathToFileURL, URL } from "node:url";

import { assertSameDraftReceipt, expectedDraftAssetNames } from "./public-draft-assets.mjs";

export const REQUIRED_UPDATE_TARGETS = Object.freeze([
  "darwin-aarch64",
  "windows-x86_64",
]);
export const NORMAL_CHECKPOINTS = Object.freeze([
  "baseline.signed_package_installed",
  "baseline.exact_installed_app_launched",
  "update.fixed_endpoint_checked",
  "update.draft_manifest_received",
  "update.draft_package_downloaded",
  "update.signature_verified",
  "update.durable_staging_observed",
  "shutdown.normal_close_requested",
  "shutdown.shared_stop_completed",
  "install.same_location_replaced",
  "install.no_automatic_relaunch",
  "candidate.manual_relaunch",
  "candidate.version_observed",
]);
export const BOOTSTRAP_CHECKPOINTS = Object.freeze([
  "bootstrap.signed_installer_verified",
  "bootstrap.candidate_installed",
  "bootstrap.exact_installed_app_launched",
  "bootstrap.version_observed",
  "bootstrap.trusted_configuration_observed",
  "bootstrap.tampered_staging_rejected",
  "bootstrap.clean_relaunch_observed",
]);

const SHA256 = /^[0-9a-f]{64}$/;
const COMMIT = /^[0-9a-f]{40}$/;
const TAG = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const MAX_EVIDENCE_BYTES = 256 * 1024;
const MAX_RUN_MILLISECONDS = 4 * 60 * 60 * 1000;
const CLOCK_SKEW_MILLISECONDS = 5 * 60 * 1000;
const TARGET_CONTRACTS = Object.freeze({
  "darwin-aarch64": Object.freeze({
    runner: Object.freeze({ os: "macos", arch: "aarch64", image: "macos-15" }),
    platform: Object.freeze({
      packageKind: "app-tar-gz",
      installerKind: "dmg",
      installScope: "user",
      codeSigning: "verified",
      notarization: "verified",
    }),
  }),
  "windows-x86_64": Object.freeze({
    runner: Object.freeze({ os: "windows", arch: "x86_64", image: "windows-2025" }),
    platform: Object.freeze({
      packageKind: "nsis-exe",
      installerKind: "nsis-exe",
      installScope: "current-user",
      registryRoot: "HKCU",
      msiPresent: false,
      authenticode: "not-required",
    }),
  }),
});

/** 要求值是普通 JSON 对象。 */
function requireObject(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} 必须是对象`);
  }
  return value;
}

/** 要求值是无换行、无 NUL 的有界字符串。 */
function requireString(value, label, maximum = 512) {
  if (typeof value !== "string" || !value || value.length > maximum || /[\r\n\0]/.test(value)) {
    throw new Error(`${label} 必须是有界单行字符串`);
  }
  return value;
}

/** 递归比较证据中的固定 runner/platform contract。 */
function requireContract(actual, expected, label) {
  const object = requireObject(actual, label);
  if (Object.keys(object).length !== Object.keys(expected).length) {
    throw new Error(`${label} 字段集合不精确`);
  }
  for (const [key, value] of Object.entries(expected)) {
    if (object[key] !== value) throw new Error(`${label}.${key} 不匹配`);
  }
}

/** 解析严格稳定三段版本。 */
function semver(value, label) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(value ?? "");
  if (!match) throw new Error(`${label} 不是稳定三段版本`);
  return match.slice(1).map(Number);
}

/** 判断 candidate 是否严格高于 previous。 */
function isNewer(candidate, previous) {
  for (let index = 0; index < 3; index += 1) {
    if (candidate[index] !== previous[index]) return candidate[index] > previous[index];
  }
  return false;
}

/** 校验 evidence 中完整的八资产 receipt。 */
function verifyDraftReceipt(receipt, expectations) {
  const object = requireObject(receipt, "draftReceipt");
  if (
    object.repository !== "cyunlab/narrant" ||
    object.releaseId !== expectations.publicReleaseId ||
    object.tag !== expectations.publicTag ||
    object.version !== expectations.version ||
    object.manifestSha256 !== expectations.manifestSha256
  ) {
    throw new Error("evidence Draft receipt 与候选不匹配");
  }
  const assets = object.assets;
  if (!Array.isArray(assets) || assets.length !== 8) throw new Error("evidence 必须绑定八个 Draft assets");
  const names = assets.map((asset) => requireString(requireObject(asset, "draft asset").name, "draft asset name", 200));
  const expectedNames = expectedDraftAssetNames(expectations.version).sort();
  if (new Set(names).size !== 8 || [...names].sort().some((name, index) => name !== expectedNames[index])) {
    throw new Error("evidence Draft asset 名称集合不精确");
  }
  for (const asset of assets) {
    if (!Number.isSafeInteger(asset.id) || asset.id <= 0) throw new Error("Draft asset ID 无效");
    if (!Number.isSafeInteger(asset.size) || asset.size <= 0) throw new Error("Draft asset size 无效");
    if (!SHA256.test(asset.sha256)) throw new Error("Draft asset SHA-256 无效");
  }
  if (expectations.draftReceipt) assertSameDraftReceipt(object, expectations.draftReceipt);
}

/** 校验两把 updater 公钥的摘要身份与当前 normal/bootstrap bridge 语义。 */
function verifyUpdaterTrust(trust, mode, receiptTrust) {
  const object = requireObject(trust, "updaterTrust");
  const fields = ["releaseSigningKeySha256", "candidateUpdaterKeySha256"];
  if (Object.keys(object).sort().join(",") !== [...fields, "bridgeAuthorized"].sort().join(",") ||
      fields.some((field) => !SHA256.test(object[field])) || typeof object.bridgeAuthorized !== "boolean") {
    throw new Error("updater key identity 无效");
  }
  const differs = object.releaseSigningKeySha256 !== object.candidateUpdaterKeySha256;
  if (differs !== object.bridgeAuthorized) throw new Error("updater key identity/bridge 状态不一致");
  if (mode === "bootstrap" && differs) throw new Error("bootstrap 不允许 updater key bridge");
  if (JSON.stringify(object) !== JSON.stringify(receiptTrust)) throw new Error("updater key identity 与 Draft receipt 不匹配");
}

/** 校验一个 target 的 evidence document。 */
export function verifyUpdateSmokeEvidenceDocument(document, expectations) {
  requireObject(document, "evidence");
  if (document.schemaVersion !== 1 || !["normal", "bootstrap"].includes(document.mode)) {
    throw new Error("evidence schema/mode 无效");
  }
  if (expectations.mode && document.mode !== expectations.mode) throw new Error("evidence mode 不匹配");
  if (document.evidenceKind !== "real-native") throw new Error("promotion 只接受 real-native evidence");
  const target = requireString(document.target, "target", 64);
  const contract = TARGET_CONTRACTS[target];
  if (!contract) throw new Error(`不支持的 evidence target：${target}`);
  requireContract(document.runner, contract.runner, "runner");
  requireContract(document.platform, contract.platform, "platform");

  const source = requireObject(document.source, "source");
  if (source.repository !== "XLCYun/narrant" || source.tag !== expectations.sourceTag || source.sha !== expectations.sourceSha) {
    throw new Error("evidence source identity 不匹配");
  }
  if (!TAG.test(source.tag) || !COMMIT.test(source.sha)) throw new Error("source tag/SHA 格式无效");
  const candidate = requireObject(document.candidate, "candidate");
  if (
    candidate.version !== expectations.version ||
    candidate.publicReleaseId !== expectations.publicReleaseId ||
    candidate.publicTag !== expectations.publicTag ||
    candidate.manifestSha256 !== expectations.manifestSha256
  ) {
    throw new Error("candidate identity 不匹配");
  }
  if (!SHA256.test(candidate.manifestSha256)) throw new Error("candidate manifest SHA-256 无效");
  verifyDraftReceipt(document.draftReceipt, expectations);
  verifyUpdaterTrust(document.updaterTrust, document.mode, document.draftReceipt.updaterTrust);

  if (document.mode === "bootstrap") {
    if (expectations.allowBootstrap !== true) throw new Error("bootstrap evidence 未获一次性授权");
    if (expectations.bootstrapTag !== source.tag) throw new Error("bootstrap tag 不匹配受保护变量");
    if (document.previousStable !== null) throw new Error("bootstrap 不得伪造 previous Stable");
  } else {
    const previous = requireObject(document.previousStable, "previousStable");
    if (!TAG.test(previous.tag) || previous.tag.replace(/^v/, "") !== previous.version) {
      throw new Error("previous Stable tag/version 无效");
    }
    if (!SHA256.test(previous.manifestSha256)) throw new Error("previous Stable manifest SHA-256 无效");
    if (!SHA256.test(previous.packageSha256) || !SHA256.test(previous.signatureSha256)) {
      throw new Error("previous Stable package/signature SHA-256 无效");
    }
    let previousUrl;
    try { previousUrl = new URL(previous.packageUrl); } catch { throw new Error("previous Stable package URL 无效"); }
    if (
      previousUrl.href !== previous.packageUrl || previousUrl.protocol !== "https:" ||
      previousUrl.hostname !== "github.com" || previousUrl.username || previousUrl.password ||
      !previousUrl.pathname.startsWith(`/cyunlab/narrant/releases/download/${encodeURIComponent(previous.tag)}/`)
    ) {
      throw new Error("previous Stable package URL 不属于固定 public Release tag");
    }
    if (expectations.previousTag && previous.tag !== expectations.previousTag) throw new Error("previous Stable tag 不匹配");
    if (expectations.previousManifestSha256 && previous.manifestSha256 !== expectations.previousManifestSha256) {
      throw new Error("previous Stable manifest SHA-256 不匹配");
    }
    if (!isNewer(semver(candidate.version, "candidate version"), semver(previous.version, "previous version"))) {
      throw new Error("candidate 必须严格高于 previous Stable");
    }
  }

  const started = new Date(document.startedAt);
  const completed = new Date(document.completedAt);
  const now = expectations.now instanceof Date ? expectations.now : new Date();
  if (Number.isNaN(started.valueOf()) || Number.isNaN(completed.valueOf())) throw new Error("evidence 时间无效");
  if (completed < started || completed.valueOf() - started.valueOf() > MAX_RUN_MILLISECONDS) {
    throw new Error("evidence 执行窗口无效");
  }
  if (completed.valueOf() > now.valueOf() + CLOCK_SKEW_MILLISECONDS) throw new Error("evidence 来自未来");
  if (now.valueOf() - completed.valueOf() > expectations.maxAgeHours * 60 * 60 * 1000) {
    throw new Error("evidence 已过期");
  }
  const required = document.mode === "bootstrap" ? BOOTSTRAP_CHECKPOINTS : NORMAL_CHECKPOINTS;
  if (!Array.isArray(document.checkpoints)) throw new Error("checkpoints 必须是数组");
  const checkpointNames = document.checkpoints.map((checkpoint) =>
    requireString(requireObject(checkpoint, "checkpoint").id, "checkpoint.id", 160));
  if (
    new Set(checkpointNames).size !== checkpointNames.length ||
    checkpointNames.length !== required.length ||
    required.some((name) => !checkpointNames.includes(name))
  ) {
    throw new Error("checkpoint 集合不精确");
  }
  for (const checkpoint of document.checkpoints) {
    if (checkpoint.status !== "passed" || Object.keys(checkpoint).some((key) => !["id", "status"].includes(key))) {
      throw new Error(`checkpoint 未通过或含额外字段：${checkpoint.id}`);
    }
  }
  return target;
}

/** 聚合严格两个 target，拒绝重复、缺少、额外和跨 target identity 漂移。 */
export function verifyUpdateSmokeEvidenceSet(documents, expectations) {
  if (!Array.isArray(documents) || !Number.isFinite(expectations?.maxAgeHours) || expectations.maxAgeHours <= 0) {
    throw new Error("evidence set 或 freshness 配置无效");
  }
  const targets = documents.map((document) => verifyUpdateSmokeEvidenceDocument(document, expectations));
  if (
    new Set(targets).size !== targets.length ||
    targets.length !== REQUIRED_UPDATE_TARGETS.length ||
    REQUIRED_UPDATE_TARGETS.some((target) => !targets.includes(target))
  ) {
    throw new Error("evidence 必须恰好覆盖两个 production targets");
  }
  const first = documents[0];
  for (const document of documents.slice(1)) {
    assertSameDraftReceipt(document.draftReceipt, first.draftReceipt);
    if (JSON.stringify(document.source) !== JSON.stringify(first.source) ||
        JSON.stringify(document.candidate) !== JSON.stringify(first.candidate) ||
        JSON.stringify(document.updaterTrust) !== JSON.stringify(first.updaterTrust) ||
        document.mode !== first.mode) {
      throw new Error("两个 target 的候选/previous identity 不一致");
    }
    if (first.mode === "normal") {
      for (const field of ["tag", "version", "manifestSha256"]) {
        if (document.previousStable[field] !== first.previousStable[field]) {
          throw new Error(`两个 target 的 previous Stable ${field} 不一致`);
        }
      }
    }
  }
  return Object.freeze({
    schemaVersion: 1,
    mode: first.mode,
    targets: REQUIRED_UPDATE_TARGETS,
    source: first.source,
    candidate: first.candidate,
    updaterTrust: first.updaterTrust,
    draftReceipt: first.draftReceipt,
    previousStable: first.previousStable,
  });
}

/** 写入 deterministic JSON evidence 与 byte-exact checksum companion。 */
export async function writeUpdateSmokeEvidence(document, directory) {
  await mkdir(directory, { recursive: true });
  const filename = `${document.target}.json`;
  const body = Buffer.from(`${JSON.stringify(document, null, 2)}\n`);
  if (body.length <= 0 || body.length > MAX_EVIDENCE_BYTES) throw new Error("evidence 文件超过安全上限");
  const checksum = `${createHash("sha256").update(body).digest("hex")}  ${filename}\n`;
  await writeFile(join(directory, filename), body, { flag: "wx", mode: 0o600 });
  await writeFile(join(directory, `${filename}.sha256`), checksum, { flag: "wx", mode: 0o600 });
  return { evidencePath: join(directory, filename), checksumPath: join(directory, `${filename}.sha256`) };
}

/** 读取只有两个 JSON 与两个 checksum companion 的证据目录。 */
export async function verifyUpdateSmokeEvidenceDirectory(directory, expectations) {
  const entries = await readdir(directory, { withFileTypes: true });
  const expected = REQUIRED_UPDATE_TARGETS.flatMap((target) => [`${target}.json`, `${target}.json.sha256`]).sort();
  const actual = entries.map((entry) => entry.name).sort();
  if (entries.some((entry) => !entry.isFile()) || actual.length !== expected.length || actual.some((name, index) => name !== expected[index])) {
    throw new Error("evidence directory 文件集合不精确");
  }
  const documents = [];
  for (const target of REQUIRED_UPDATE_TARGETS) {
    const filename = `${target}.json`;
    const evidencePath = join(directory, filename);
    const information = await stat(evidencePath);
    if (information.size <= 0 || information.size > MAX_EVIDENCE_BYTES) throw new Error(`${filename} 大小无效`);
    const [body, checksum] = await Promise.all([
      readFile(evidencePath),
      readFile(`${evidencePath}.sha256`, "utf8"),
    ]);
    const expectedChecksum = `${createHash("sha256").update(body).digest("hex")}  ${filename}\n`;
    if (checksum !== expectedChecksum) throw new Error(`${filename} checksum 不匹配`);
    try { documents.push(JSON.parse(body.toString("utf8"))); }
    catch { throw new Error(`${filename} 不是有效 JSON`); }
  }
  return verifyUpdateSmokeEvidenceSet(documents, expectations);
}

/** 将 CLI 成对参数与单一布尔 flag 转成 verifier options。 */
function parseArguments(arguments_) {
  const values = {};
  for (let index = 0; index < arguments_.length; index += 1) {
    const name = arguments_[index];
    if (name === "--allow-bootstrap") { values.allowBootstrap = true; continue; }
    const value = arguments_[index + 1];
    if (!name?.startsWith("--") || value === undefined) throw new Error(`无效参数：${name ?? ""}`);
    values[name.slice(2)] = value;
    index += 1;
  }
  return values;
}

/** workflow 使用的公开 verifier CLI。 */
export async function runEvidenceVerifierCli(arguments_ = process.argv.slice(2), now = new Date()) {
  const values = parseArguments(arguments_);
  for (const name of ["evidence", "mode", "source-tag", "source-sha", "public-release-id", "public-tag", "version", "manifest-sha256", "max-age-hours"]) {
    if (!values[name]) throw new Error(`缺少 --${name}`);
  }
  const draftReceipt = values["draft-receipt"]
    ? JSON.parse(await readFile(values["draft-receipt"], "utf8"))
    : undefined;
  return verifyUpdateSmokeEvidenceDirectory(values.evidence, {
    mode: values.mode,
    sourceTag: values["source-tag"],
    sourceSha: values["source-sha"],
    publicReleaseId: Number(values["public-release-id"]),
    publicTag: values["public-tag"],
    version: values.version,
    manifestSha256: values["manifest-sha256"],
    draftReceipt,
    previousTag: values["previous-tag"],
    previousManifestSha256: values["previous-manifest-sha256"],
    bootstrapTag: values["bootstrap-tag"],
    allowBootstrap: values.allowBootstrap === true,
    maxAgeHours: Number(values["max-age-hours"]),
    now,
  });
}

async function main() {
  const result = await runEvidenceVerifierCli();
  process.stdout.write(`已验证 ${result.mode} evidence（${result.targets.length} 个 targets）。\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`错误：${error instanceof Error ? error.message : "evidence 验证失败"}\n`);
    process.exitCode = 1;
  });
}

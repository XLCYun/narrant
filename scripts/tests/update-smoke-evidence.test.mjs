import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { expectedDraftAssetNames } from "../public-draft-assets.mjs";
import {
  BOOTSTRAP_CHECKPOINTS,
  NORMAL_CHECKPOINTS,
  verifyUpdateSmokeEvidenceDirectory,
  verifyUpdateSmokeEvidenceDocument,
  verifyUpdateSmokeEvidenceSet,
  writeUpdateSmokeEvidence,
} from "../update-smoke-evidence.mjs";

const now = new Date("2026-09-02T08:00:00.000Z");
const sha = "a".repeat(64);
const sourceSha = "b".repeat(40);

test("自托管双平台证据保留真实主机信息，仍拒绝错误平台或不完整身份", () => {
  const documents = ["darwin-aarch64", "windows-x86_64"].map(target => {
    const document = evidence(target);
    document.runner = { ...document.runner, image: "self-hosted", name: "isolated-ci",
      osVersion: target === "darwin-aarch64" ? "Darwin Kernel Version 26" : "Windows 10 Pro",
      osRelease: target === "darwin-aarch64" ? "26.0.0" : "10.0.19045" };
    return document;
  });
  assert.doesNotThrow(() => verifyUpdateSmokeEvidenceSet(documents, expectations()));
  for (const field of ["name", "osVersion", "osRelease"]) {
    const invalid = structuredClone(documents[0]);
    delete invalid.runner[field];
    assert.throws(() => verifyUpdateSmokeEvidenceDocument(invalid, expectations()), /runner/);
  }
  const invalid = structuredClone(documents[0]);
  invalid.runner.os = "windows";
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(invalid, expectations()), /runner.os/);
  invalid.runner = { os: "macos", arch: "aarch64", image: "unverified-image" };
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(invalid, expectations()), /runner.image/);
});

/** 生成固定八资产 receipt。 */
function receipt() {
  return {
    schemaVersion: 1,
    repository: "cyunlab/narrant",
    releaseId: 42,
    tag: "v2.0.0",
    version: "2.0.0",
    manifestSha256: sha,
    updaterTrust: {
      releaseSigningKeySha256: "d".repeat(64),
      candidateUpdaterKeySha256: "d".repeat(64),
      bridgeAuthorized: false,
    },
    assets: expectedDraftAssetNames("2.0.0").sort().map((name, index) => ({
      id: index + 1, name, size: index + 10, sha256: String(index + 1).padStart(64, "0"),
    })),
  };
}

/** 创建一个 promotion-eligible target document。 */
function evidence(target, mode = "normal") {
  const mac = target === "darwin-aarch64";
  return {
    schemaVersion: 1,
    evidenceKind: "real-native",
    mode,
    target,
    runner: mac
      ? { os: "macos", arch: "aarch64", image: "macos-15" }
      : { os: "windows", arch: "x86_64", image: "windows-2025" },
    platform: mac
      ? { packageKind: "app-tar-gz", installerKind: "dmg", installScope: "user", codeSigning: "verified", notarization: "verified" }
      : { packageKind: "nsis-exe", installerKind: "nsis-exe", installScope: "current-user", registryRoot: "HKCU", msiPresent: false, authenticode: "not-required" },
    source: { repository: "XLCYun/narrant", tag: "v2.0.0", sha: sourceSha },
    candidate: { version: "2.0.0", publicReleaseId: 42, publicTag: "v2.0.0", manifestSha256: sha },
    updaterTrust: receipt().updaterTrust,
    draftReceipt: receipt(),
    previousStable: mode === "bootstrap" ? null : {
      tag: "v1.9.0", version: "1.9.0", manifestSha256: "c".repeat(64),
      packageSha256: mac ? "d".repeat(64) : "e".repeat(64),
      signatureSha256: mac ? "f".repeat(64) : "1".repeat(64),
      previousReleaseSigningKeySha256: "2".repeat(64),
      packageUrl: `https://github.com/cyunlab/narrant/releases/download/v1.9.0/previous-${mac ? "mac" : "windows"}`,
    },
    startedAt: "2026-09-02T07:00:00.000Z",
    completedAt: "2026-09-02T07:10:00.000Z",
    checkpoints: (mode === "bootstrap" ? BOOTSTRAP_CHECKPOINTS : NORMAL_CHECKPOINTS).map((id) => ({ id, status: "passed" })),
  };
}

/** 返回固定 expectations。 */
function expectations(mode = "normal") {
  return {
    mode,
    sourceTag: "v2.0.0",
    sourceSha,
    publicReleaseId: 42,
    publicTag: "v2.0.0",
    version: "2.0.0",
    manifestSha256: sha,
    draftReceipt: receipt(),
    previousTag: mode === "normal" ? "v1.9.0" : undefined,
    previousManifestSha256: mode === "normal" ? "c".repeat(64) : undefined,
    previousReleaseSigningKeySha256: mode === "normal" ? "2".repeat(64) : undefined,
    allowBootstrap: mode === "bootstrap",
    bootstrapTag: mode === "bootstrap" ? "v2.0.0" : undefined,
    maxAgeHours: 24,
    now,
  };
}

test("normal evidence 绑定源码、Draft 八资产、previous package/signature 与真实 checkpoints", () => {
  assert.equal(verifyUpdateSmokeEvidenceDocument(evidence("darwin-aarch64"), expectations()), "darwin-aarch64");
  assert.equal(verifyUpdateSmokeEvidenceSet([
    evidence("darwin-aarch64"), evidence("windows-x86_64"),
  ], expectations()).targets.length, 2);
});

test("evidence 绑定两个 updater key identity，并只允许 normal bridge", () => {
  const bridged = evidence("darwin-aarch64");
  bridged.updaterTrust.candidateUpdaterKeySha256 = "e".repeat(64);
  bridged.updaterTrust.bridgeAuthorized = true;
  bridged.draftReceipt.updaterTrust = bridged.updaterTrust;
  const bridgedExpectations = expectations();
  bridgedExpectations.draftReceipt.updaterTrust = bridged.updaterTrust;
  assert.doesNotThrow(() => verifyUpdateSmokeEvidenceDocument(bridged, bridgedExpectations));

  const mismatched = JSON.parse(JSON.stringify(bridged));
  mismatched.updaterTrust.releaseSigningKeySha256 = "f".repeat(64);
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(mismatched, bridgedExpectations), /updater key identity/);
  const bootstrap = evidence("darwin-aarch64", "bootstrap");
  bootstrap.updaterTrust.candidateUpdaterKeySha256 = "e".repeat(64);
  bootstrap.updaterTrust.bridgeAuthorized = true;
  bootstrap.draftReceipt.updaterTrust = bootstrap.updaterTrust;
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(bootstrap, {
    ...expectations("bootstrap"), draftReceipt: bootstrap.draftReceipt,
  }), /bootstrap.*bridge/i);
});

test("evidence set 拒绝重复、缺少、额外目标与跨 target Draft 漂移", () => {
  const mac = evidence("darwin-aarch64");
  const windows = evidence("windows-x86_64");
  assert.throws(() => verifyUpdateSmokeEvidenceSet([mac], expectations()), /两个 production targets/);
  assert.throws(() => verifyUpdateSmokeEvidenceSet([mac, mac], expectations()), /两个 production targets/);
  windows.draftReceipt.assets[0].id = 999;
  assert.throws(() => verifyUpdateSmokeEvidenceSet([mac, windows], expectations()), /Draft asset IDs/);
});

test("normal evidence 精确绑定 previous release key hash 且两 target 必须一致", () => {
  const mac = evidence("darwin-aarch64");
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(mac, {
    ...expectations(), previousReleaseSigningKeySha256: "3".repeat(64),
  }), /previous release signing key/i);

  const windows = evidence("windows-x86_64");
  windows.previousStable.previousReleaseSigningKeySha256 = "3".repeat(64);
  assert.throws(
    () => verifyUpdateSmokeEvidenceSet([mac, windows], expectations()),
    /previous Stable previousReleaseSigningKeySha256|previous release signing key/i,
  );
});

test("bootstrap 必须显式授权、匹配 protected tag 且没有 previous Stable", () => {
  const bootstrap = evidence("darwin-aarch64", "bootstrap");
  assert.doesNotThrow(() => verifyUpdateSmokeEvidenceDocument(bootstrap, expectations("bootstrap")));
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(bootstrap, { ...expectations("bootstrap"), allowBootstrap: false }), /未获一次性授权/);
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(bootstrap, { ...expectations("bootstrap"), bootstrapTag: "v1.0.0" }), /bootstrap tag/);
  bootstrap.previousStable = evidence("darwin-aarch64").previousStable;
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(bootstrap, expectations("bootstrap")), /不得伪造/);
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(evidence("darwin-aarch64"), expectations("bootstrap")), /mode/);
});

test("freshness、future、checkpoint 和 source SHA 均 fail closed", () => {
  const stale = evidence("darwin-aarch64");
  stale.startedAt = "2026-08-30T23:50:00.000Z";
  stale.completedAt = "2026-08-31T00:00:00.000Z";
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(stale, expectations()), /过期/);
  const future = evidence("darwin-aarch64");
  future.completedAt = "2026-09-02T09:00:00.000Z";
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(future, expectations()), /未来/);
  const missing = evidence("darwin-aarch64");
  missing.checkpoints.pop();
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(missing, expectations()), /checkpoint 集合/);
  const wrongSource = evidence("darwin-aarch64");
  wrongSource.source.sha = "A".repeat(40);
  assert.throws(() => verifyUpdateSmokeEvidenceDocument(wrongSource, expectations()), /source identity/);
});

test("evidence directory 要求 byte-exact checksum companion 与四个固定文件", async (context) => {
  const directory = await mkdtemp(join(tmpdir(), "narrant-evidence-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  await writeUpdateSmokeEvidence(evidence("darwin-aarch64"), directory);
  await writeUpdateSmokeEvidence(evidence("windows-x86_64"), directory);
  assert.equal((await verifyUpdateSmokeEvidenceDirectory(directory, expectations())).targets.length, 2);
  const checksum = join(directory, "darwin-aarch64.json.sha256");
  await writeFile(checksum, (await readFile(checksum, "utf8")).replace(/^./, "0"));
  await assert.rejects(() => verifyUpdateSmokeEvidenceDirectory(directory, expectations()), /checksum/);
});

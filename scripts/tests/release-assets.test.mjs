import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import {
  generateKeyPairSync,
  sign,
} from "node:crypto";
import {
  cpSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { gzipSync } from "node:zlib";

import {
  assertDraftArtifactPaths,
  parseReleaseIdentity,
  prepareDraftAssetDirectory,
  releaseAssetNames,
  validateUpdaterManifest,
  verifyTauriUpdaterSignature,
} from "../release-assets.mjs";

/** 创建含 PE 签名字节的最小 Windows 容器 fixture。 */
function createPeFixture(marker) {
  const bytes = Buffer.alloc(160, marker);
  bytes.write("MZ", 0, "ascii");
  bytes.writeUInt32LE(64, 60);
  bytes.write("PE\0\0", 64, "ascii");
  return bytes;
}

/** 创建带 UDIF 尾部魔数的最小 DMG fixture。 */
function createDmgFixture() {
  const bytes = Buffer.alloc(1024, 3);
  bytes.write("koly", bytes.length - 512, "ascii");
  return bytes;
}

/** 创建首个条目为 Narrant.app 的最小 gzip tar fixture。 */
function createAppArchiveFixture() {
  const tar = Buffer.alloc(1024);
  tar.write("Narrant.app/", 0, "ascii");
  tar.write("ustar", 257, "ascii");
  return gzipSync(tar);
}

/** 创建与 Tauri minisign 外层 base64 格式兼容的本地 Ed25519 fixture。 */
function createSigningFixture(packages) {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const rawPublicKey = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const id = Buffer.from("0102030405060708", "hex");
  const publicPacket = Buffer.concat([Buffer.from("Ed"), id, rawPublicKey]);
  const publicText = [
    "untrusted comment: narrant test key",
    publicPacket.toString("base64"),
  ].join("\n");
  const encodedPublicKey = Buffer.from(publicText).toString("base64");
  const signatures = new Map();
  for (const [name, bytes] of packages) {
    const packageSignature = sign(null, bytes, privateKey);
    const comment = `fixture ${name}`;
    const globalSignature = sign(
      null,
      Buffer.concat([packageSignature, Buffer.from(comment)]),
      privateKey,
    );
    const signaturePacket = Buffer.concat([Buffer.from("Ed"), id, packageSignature]);
    const signatureText = [
      "untrusted comment: narrant test signature",
      signaturePacket.toString("base64"),
      `trusted comment: ${comment}`,
      globalSignature.toString("base64"),
    ].join("\n");
    signatures.set(name, `${Buffer.from(signatureText).toString("base64")}\n`);
  }
  return { publicKey: encodedPublicKey, signatures };
}

/** 在临时目录写入六个原生产物，并返回可重复使用的测试上下文。 */
function createNativeAssetFixture(version = "2.3.4") {
  const directory = mkdtempSync(join(tmpdir(), "narrant-release-assets-"));
  const names = releaseAssetNames(version);
  const packages = new Map([
    [names.updater.darwin, createAppArchiveFixture()],
    [names.updater.windows, createPeFixture(7)],
  ]);
  const signing = createSigningFixture(packages);
  writeFileSync(join(directory, names.installer.darwin), createDmgFixture());
  writeFileSync(join(directory, names.installer.windows), createPeFixture(9));
  for (const [name, bytes] of packages) {
    writeFileSync(join(directory, name), bytes);
    writeFileSync(join(directory, `${name}.sig`), signing.signatures.get(name));
  }
  return { directory, names, ...signing };
}

/** 运行完整资产准备，并确保测试结束清理临时目录。 */
function prepareFixture(fixture, overrides = {}) {
  return prepareDraftAssetDirectory({
    directory: fixture.directory,
    tag: "v2.3.4",
    version: "2.3.4",
    notes: "发布说明\n保留换行。\n",
    publicKey: fixture.publicKey,
    privateRepository: "cyunlab/narrant-private",
    sourceSha: "a".repeat(40),
    ...overrides,
  });
}

test("稳定 tag/version 解析严格且可重复", () => {
  assert.deepEqual(parseReleaseIdentity("v2.3.4", "2.3.4"), {
    tag: "v2.3.4",
    version: "2.3.4",
  });
  assert.throws(() => parseReleaseIdentity("2.3.4", "2.3.4"), /不匹配/);
  assert.throws(() => parseReleaseIdentity("v2.3.5", "2.3.4"), /不匹配/);
});

test("完整准备生成固定两目标 manifest 并字面保留签名文本", (context) => {
  const fixture = createNativeAssetFixture();
  context.after(() => rmSync(fixture.directory, { recursive: true, force: true }));
  const result = prepareFixture(fixture);
  const manifest = JSON.parse(readFileSync(join(fixture.directory, "latest.json"), "utf8"));

  assert.deepEqual(Object.keys(manifest.platforms), ["darwin-aarch64", "windows-x86_64"]);
  assert.equal(manifest.pub_date, undefined);
  assert.equal(manifest.notes, "发布说明\n保留换行。\n");
  assert.equal(
    manifest.platforms["darwin-aarch64"].signature,
    fixture.signatures.get(fixture.names.updater.darwin),
  );
  assert.match(
    manifest.platforms["windows-x86_64"].url,
    /cyunlab\/narrant\/releases\/download\/v2\.3\.4\/narrant-windows-x86_64-updater\.exe$/,
  );
  assert.equal(result.names.length, 8);
});

test("updater 验签接受有效签名并拒绝包篡改、签名损坏和错误公钥", () => {
  const packageBytes = createPeFixture(5);
  const signing = createSigningFixture(new Map([["updater.exe", packageBytes]]));
  const signature = signing.signatures.get("updater.exe");
  assert.doesNotThrow(() => verifyTauriUpdaterSignature(packageBytes, signature, signing.publicKey));

  const tampered = Buffer.from(packageBytes);
  tampered[100] ^= 1;
  assert.throws(
    () => verifyTauriUpdaterSignature(tampered, signature, signing.publicKey),
    /包签名无效/,
  );
  assert.throws(
    () => verifyTauriUpdaterSignature(packageBytes, "not-base64", signing.publicKey),
    /签名/,
  );
  const other = createSigningFixture(new Map([["other.exe", packageBytes]]));
  assert.throws(
    () => verifyTauriUpdaterSignature(packageBytes, signature, other.publicKey),
    /key id|包签名/,
  );
});

test("资产集合拒绝缺少、意外与重复名称", (context) => {
  const missing = createNativeAssetFixture();
  const unexpected = createNativeAssetFixture();
  context.after(() => rmSync(missing.directory, { recursive: true, force: true }));
  context.after(() => rmSync(unexpected.directory, { recursive: true, force: true }));
  unlinkSync(join(missing.directory, missing.names.installer.darwin));
  assert.throws(() => prepareFixture(missing), /缺少/);
  writeFileSync(join(unexpected.directory, "unexpected.txt"), "no");
  assert.throws(() => prepareFixture(unexpected), /意外/);

  const exactPaths = [
    `a/${unexpected.names.installer.darwin}`,
    `a/${unexpected.names.installer.windows}`,
    `a/${unexpected.names.updater.darwin}`,
    `a/${unexpected.names.updater.darwin}.sig`,
    `a/${unexpected.names.updater.windows}`,
    `a/${unexpected.names.updater.windows}.sig`,
    "a/latest.json",
    "a/SHA256SUMS",
  ];
  exactPaths[1] = exactPaths[0];
  assert.throws(() => assertDraftArtifactPaths(exactPaths, "2.3.4"), /重复.*缺少/);
});

test("manifest 校验拒绝 URL、名称、版本、notes 和签名映射偏差", (context) => {
  const fixture = createNativeAssetFixture();
  context.after(() => rmSync(fixture.directory, { recursive: true, force: true }));
  prepareFixture(fixture);
  const manifest = JSON.parse(readFileSync(join(fixture.directory, "latest.json"), "utf8"));
  const signatures = {
    [`${fixture.names.updater.darwin}.sig`]: fixture.signatures.get(fixture.names.updater.darwin),
    [`${fixture.names.updater.windows}.sig`]: fixture.signatures.get(fixture.names.updater.windows),
  };
  const options = {
    tag: "v2.3.4",
    version: "2.3.4",
    notes: "发布说明\n保留换行。\n",
    signatures,
  };
  assert.doesNotThrow(() => validateUpdaterManifest(manifest, options));
  for (const mutate of [
    (copy) => { copy.platforms["darwin-aarch64"].url = "https://example.com/wrong"; },
    (copy) => { copy.platforms["darwin-aarch64"].url = copy.platforms["darwin-aarch64"].url.replace("darwin", "macos"); },
    (copy) => { copy.version = "2.3.5"; },
    (copy) => { copy.notes = "other"; },
    (copy) => { copy.platforms["windows-x86_64"].signature = "other"; },
  ]) {
    const copy = JSON.parse(JSON.stringify(manifest));
    mutate(copy);
    assert.throws(() => validateUpdaterManifest(copy, options), /latest\.json/);
  }
});

test("SHA256SUMS 只覆盖七文件且按公开名称稳定排序", (context) => {
  const first = createNativeAssetFixture();
  const second = createNativeAssetFixture();
  context.after(() => rmSync(first.directory, { recursive: true, force: true }));
  context.after(() => rmSync(second.directory, { recursive: true, force: true }));
  // 两个目录使用不同随机密钥；复制相同六文件后应生成逐字节相同结果。
  for (const name of [
    first.names.installer.darwin,
    first.names.installer.windows,
    first.names.updater.darwin,
    `${first.names.updater.darwin}.sig`,
    first.names.updater.windows,
    `${first.names.updater.windows}.sig`,
  ]) {
    cpSync(join(first.directory, name), join(second.directory, name));
  }
  second.publicKey = first.publicKey;
  prepareFixture(first);
  prepareFixture(second);
  const firstSums = readFileSync(join(first.directory, "SHA256SUMS"), "utf8");
  const secondSums = readFileSync(join(second.directory, "SHA256SUMS"), "utf8");
  const covered = firstSums.trimEnd().split("\n").map((line) => line.slice(66));
  assert.equal(firstSums, secondSums);
  assert.equal(covered.length, 7);
  assert.deepEqual(covered, [
    "Narrant_2.3.4_aarch64.dmg",
    "Narrant_2.3.4_x64-setup.exe",
    "latest.json",
    "narrant-darwin-aarch64-updater.app.tar.gz",
    "narrant-darwin-aarch64-updater.app.tar.gz.sig",
    "narrant-windows-x86_64-updater.exe",
    "narrant-windows-x86_64-updater.exe.sig",
  ]);
  assert.equal(covered.includes("SHA256SUMS"), false);
});

test("公开元数据拒绝私有仓库、完整源码 SHA 和 Actions 链接", (context) => {
  const fixtures = [createNativeAssetFixture(), createNativeAssetFixture(), createNativeAssetFixture()];
  context.after(() => fixtures.forEach((fixture) => rmSync(fixture.directory, { recursive: true, force: true })));
  assert.throws(() => prepareFixture(fixtures[0], { notes: "cyunlab/narrant-private" }), /私有/);
  assert.throws(() => prepareFixture(fixtures[1], { notes: "a".repeat(40) }), /私有/);
  assert.throws(
    () => prepareFixture(fixtures[2], { notes: "https://github.com/org/repo/actions/runs/123" }),
    /Actions/,
  );
});

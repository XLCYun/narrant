import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import {
  createHash,
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

/** 生成带 padding 的 canonical outer-base64，便于覆盖缺 padding 的拒绝路径。 */
function encodeCanonicalFixture(lines) {
  const adjusted = [...lines];
  while (Buffer.byteLength(adjusted.join("\n")) % 3 === 0) {
    adjusted[0] += "x";
  }
  return Buffer.from(adjusted.join("\n")).toString("base64");
}

/** 解码后变换 minisign inner text，再重新包成 canonical outer-base64。 */
function rewriteCanonicalOuter(encoded, transform) {
  const inner = Buffer.from(encoded, "base64").toString("utf8");
  return Buffer.from(transform(inner)).toString("base64");
}

/** 创建与客户端 prehashed ED minisign 语义兼容的本地 Ed25519 fixture。 */
function createSigningFixture(packages, options = {}) {
  const algorithm = options.algorithm ?? "ED";
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const rawPublicKey = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const id = Buffer.from("0102030405060708", "hex");
  const publicPacket = Buffer.concat([Buffer.from("Ed"), id, rawPublicKey]);
  const encodedPublicKey = encodeCanonicalFixture([
    "untrusted comment: narrant test key",
    publicPacket.toString("base64"),
  ]);
  const signatures = new Map();
  for (const [name, bytes] of packages) {
    const message = algorithm === "ED"
      ? createHash("blake2b512").update(bytes).digest()
      : bytes;
    const packageSignature = sign(null, message, privateKey);
    const comment = `fixture ${name}`;
    const globalSignature = sign(
      null,
      Buffer.concat([packageSignature, Buffer.from(comment)]),
      privateKey,
    );
    const signaturePacket = Buffer.concat([Buffer.from(algorithm), id, packageSignature]);
    const signatureText = encodeCanonicalFixture([
      "untrusted comment: narrant test signature",
      signaturePacket.toString("base64"),
      `trusted comment: ${comment}`,
      globalSignature.toString("base64"),
    ]);
    signatures.set(name, signatureText);
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

test("完整准备生成固定两目标 manifest 并字面保留 canonical ED 签名文件", (context) => {
  const fixture = createNativeAssetFixture();
  context.after(() => rmSync(fixture.directory, { recursive: true, force: true }));
  const result = prepareFixture(fixture);
  const manifest = JSON.parse(readFileSync(join(fixture.directory, "latest.json"), "utf8"));

  assert.deepEqual(Object.keys(manifest.platforms), ["darwin-aarch64", "windows-x86_64"]);
  assert.equal(manifest.pub_date, undefined);
  assert.equal(manifest.notes, "发布说明\n保留换行。\n");
  const signatureFile = readFileSync(
    join(fixture.directory, `${fixture.names.updater.darwin}.sig`),
    "utf8",
  );
  assert.equal(manifest.platforms["darwin-aarch64"].signature, signatureFile);
  assert.equal(signatureFile, fixture.signatures.get(fixture.names.updater.darwin));
  assert.doesNotMatch(signatureFile, /\s/);
  assert.equal(Buffer.from(signatureFile, "base64").toString("base64"), signatureFile);
  assert.match(
    manifest.platforms["windows-x86_64"].url,
    /cyunlab\/narrant\/releases\/download\/v2\.3\.4\/narrant-windows-x86_64-updater\.exe$/,
  );
  assert.equal(result.names.length, 8);
});

test("updater 验签只接受 canonical prehashed ED，并拒绝 legacy/非 canonical/篡改", () => {
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
  const legacy = createSigningFixture(new Map([["legacy.exe", packageBytes]]), { algorithm: "Ed" });
  assert.throws(
    () => verifyTauriUpdaterSignature(packageBytes, legacy.signatures.get("legacy.exe"), legacy.publicKey),
    /prehashed ED/,
  );
  assert.throws(
    () => verifyTauriUpdaterSignature(packageBytes, `${signature}\n`, signing.publicKey),
    /canonical base64/,
  );
  assert.throws(
    () => verifyTauriUpdaterSignature(packageBytes, signature.replace(/=+$/, ""), signing.publicKey),
    /canonical base64/,
  );
  assert.throws(
    () => verifyTauriUpdaterSignature(packageBytes, signature, `${signing.publicKey} `),
    /canonical base64/,
  );
  assert.throws(
    () => verifyTauriUpdaterSignature(packageBytes, signature, signing.publicKey.replace(/=+$/, "")),
    /canonical base64/,
  );
  for (const mutateInner of [
    (inner) => `${inner} `,
    (inner) => `${inner}\n`,
    (inner) => inner.replaceAll("\n", "\r\n"),
    (inner) => {
      const lines = inner.split("\n");
      lines[2] += " ";
      return lines.join("\n");
    },
  ]) {
    assert.throws(
      () => verifyTauriUpdaterSignature(
        packageBytes,
        rewriteCanonicalOuter(signature, mutateInner),
        signing.publicKey,
      ),
      /inner text|canonical base64/,
    );
  }
  for (const mutatePublicInner of [
    (inner) => `${inner} `,
    (inner) => `${inner}\n`,
    (inner) => inner.replaceAll("\n", "\r\n"),
    (inner) => inner.replace("\n", " \n"),
  ]) {
    assert.throws(
      () => verifyTauriUpdaterSignature(
        packageBytes,
        signature,
        rewriteCanonicalOuter(signing.publicKey, mutatePublicInner),
      ),
      /inner text|canonical base64/,
    );
  }
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

test("公开元数据逐个拒绝任意 SHA 和 delimiter 拼接的 allowlist 外 URL", (context) => {
  const fixtures = Array.from({ length: 28 }, () => createNativeAssetFixture());
  context.after(() => fixtures.forEach((fixture) => rmSync(fixture.directory, { recursive: true, force: true })));
  assert.throws(() => prepareFixture(fixtures[0], { notes: "cyunlab/narrant-private" }), /私有/);
  assert.throws(() => prepareFixture(fixtures[1], { notes: "b".repeat(40) }), /完整 Git SHA/);
  assert.throws(
    () => prepareFixture(fixtures[2], { notes: "https://github.com/other/private/issues/7" }),
    /未允许 URL/,
  );
  assert.throws(
    () => prepareFixture(fixtures[3], { notes: "https://git.internal.example/review/42" }),
    /未允许 URL/,
  );
  assert.throws(
    () => prepareFixture(fixtures[4], { notes: "http://github.com/cyunlab/narrant/issues/1" }),
    /未允许 URL/,
  );
  assert.throws(
    () => prepareFixture(fixtures[5], { notes: "https://github.com/cyunlab/narrant/actions/runs/123" }),
    /未允许 URL/,
  );
  assert.doesNotThrow(() => prepareFixture(fixtures[6], {
    notes: "公开说明（https://github.com/cyunlab/narrant/issues/42）。",
  }));
  const publicUrl = "https://github.com/cyunlab/narrant/issues/42";
  for (const [fixture, notes] of [
    [fixtures[7], `${publicUrl},https://internal.example/secret`],
    [fixtures[8], `${publicUrl};https://github.com/private/repo/issues/1`],
    [fixtures[9], `${publicUrl}https://internal.example/adjacent`],
    [fixtures[10], `[公开](${publicUrl}) 与 [私有](https://internal.example/md)`],
    [fixtures[11], `https://,${publicUrl}`],
  ]) {
    assert.throws(() => prepareFixture(fixture, { notes }), /未允许 URL|无效 URL/);
  }
  assert.doesNotThrow(() => prepareFixture(fixtures[12], {
    notes: `[公开 issue](${publicUrl}), 另见 ${publicUrl};`,
  }));
  for (const [fixture, notes] of [
    [fixtures[13], `<a href="https://internal.example/admin">private</a>`],
    [fixtures[14], `<img src='//internal.example/image.png'>`],
    [fixtures[15], `参见 //internal.example/runbook`],
    [fixtures[16], `[联系](mailto:private@example.com)`],
    [fixtures[17], `custom-tool://internal/secret`],
  ]) {
    assert.throws(
      () => prepareFixture(fixture, { notes }),
      /未允许 URL|无效 URL|protocol-relative|URI scheme/,
    );
  }
  assert.doesNotThrow(() => prepareFixture(fixtures[18], {
    notes: `fix: 保留 conventional subject；[公开](${publicUrl})。`,
  }));
  assert.throws(
    () => prepareFixture(fixtures[19], { notes: "参见 //localhost/internal" }),
    /protocol-relative/,
  );
  assert.doesNotThrow(() => prepareFixture(fixtures[20], {
    notes: `<a href="${publicUrl}">公开 issue</a>`,
  }));
  for (const [fixture, notes] of [
    [fixtures[21], "内部手册 www.internal.example/runbook"],
    [fixtures[22], "联系 private@example.com"],
    [fixtures[23], "[内部][ref]\n\n[ref]: /private/runbook"],
    [fixtures[24], "![内部图][image]\n\n[image]: mailto:private@example.com"],
    [fixtures[25], "[内部](/private/runbook)"],
  ]) {
    assert.throws(
      () => prepareFixture(fixture, { notes }),
      /未允许 URL|无效 URL/,
    );
  }
  assert.doesNotThrow(() => prepareFixture(fixtures[26], {
    notes: `公开 autolink <${publicUrl}>`,
  }));
  assert.doesNotThrow(() => prepareFixture(fixtures[27], {
    notes: `[公开][issue]\n\n[issue]: ${publicUrl}`,
  }));
});

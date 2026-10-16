import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { preparePreviousStable } from "../prepare-update-smoke-assets.mjs";
import { updaterPublicKeySha256 } from "../release-assets.mjs";

/** 把 minisign inner text 编码成 canonical outer-base64。 */
function outer(lines) {
  const adjusted = [...lines];
  while (Buffer.byteLength(adjusted.join("\n")) % 3 === 0) adjusted[0] += "x";
  return Buffer.from(adjusted.join("\n")).toString("base64");
}

/** 生成带独立 key-id 的真实 prehashed ED minisign fixture。 */
function signingFixture(bytes, idHex) {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const id = Buffer.from(idHex, "hex");
  const raw = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const publicKeyText = outer([
    "untrusted comment: previous release test key",
    Buffer.concat([Buffer.from("Ed"), id, raw]).toString("base64"),
  ]);
  const packageSignature = sign(null, createHash("blake2b512").update(bytes).digest(), privateKey);
  const comment = "previous stable archive";
  const global = sign(null, Buffer.concat([packageSignature, Buffer.from(comment)]), privateKey);
  const signature = outer([
    "untrusted comment: previous release test signature",
    Buffer.concat([Buffer.from("ED"), id, packageSignature]).toString("base64"),
    `trusted comment: ${comment}`,
    global.toString("base64"),
  ]);
  return { publicKey: publicKeyText, signature };
}

/** 创建带可信 final URL 的 Fetch Response。 */
function response(body, url, type = "application/octet-stream") {
  const value = new globalThis.Response(body, { status: 200, headers: { "content-type": type } });
  Object.defineProperty(value, "url", { value: url });
  return value;
}

/** 构造 previous B archive 与双目标 manifest fetch。 */
function previousFixture() {
  const packageBytes = Buffer.from("bridge-B-archive-signed-by-old");
  const old = signingFixture(packageBytes, "0102030405060708");
  const successor = signingFixture(Buffer.from("candidate-C"), "1112131415161718");
  const packageUrl = "https://github.com/XLCYun/narrant/releases/download/v2.0.0/narrant-windows-x86_64-updater.exe";
  const manifest = {
    version: "2.0.0",
    platforms: {
      "darwin-aarch64": { url: "https://github.com/XLCYun/narrant/releases/download/v2.0.0/narrant-darwin-aarch64-updater.app.tar.gz", signature: old.signature },
      "windows-x86_64": { url: packageUrl, signature: old.signature },
    },
  };
  const manifestBytes = Buffer.from(JSON.stringify(manifest));
  const fetchImpl = async (url) => String(url) === packageUrl
    ? response(packageBytes, packageUrl)
    : response(manifestBytes, "https://github.com/XLCYun/narrant/releases/latest/download/latest.json", "application/json");
  return { packageBytes, old, successor, packageUrl, fetchImpl };
}

test("C admission 用 previous old key 验 B archive，只持久化 previous key SHA-256", async (context) => {
  const fixture = previousFixture();
  const output = await mkdtemp(join(tmpdir(), "narrant-previous-key-"));
  context.after(() => rm(output, { recursive: true, force: true }));
  const result = await preparePreviousStable({
    target: "windows-x86_64",
    expectedTag: "v2.0.0",
    outputDirectory: output,
    previousReleaseSigningPublicKey: fixture.old.publicKey,
  }, { fetchImpl: fixture.fetchImpl });
  assert.equal(
    result.previousReleaseSigningKeySha256,
    updaterPublicKeySha256(fixture.old.publicKey),
  );
  const persisted = await readFile(join(output, "previous.json"), "utf8");
  assert.equal(persisted.includes(fixture.old.publicKey), false);
  assert.equal(persisted.includes(fixture.successor.publicKey), false);
});

test("C admission 错用 successor 验 B archive稳定失败", async (context) => {
  const fixture = previousFixture();
  const output = await mkdtemp(join(tmpdir(), "narrant-wrong-previous-key-"));
  context.after(() => rm(output, { recursive: true, force: true }));
  await assert.rejects(() => preparePreviousStable({
    target: "windows-x86_64",
    expectedTag: "v2.0.0",
    outputDirectory: output,
    previousReleaseSigningPublicKey: fixture.successor.publicKey,
  }, { fetchImpl: fixture.fetchImpl }), /key id 不匹配/);
});

test("normal previous key 缺失时在下载前 fail closed", async (context) => {
  const output = await mkdtemp(join(tmpdir(), "narrant-missing-previous-key-"));
  context.after(() => rm(output, { recursive: true, force: true }));
  let calls = 0;
  await assert.rejects(() => preparePreviousStable({
    target: "windows-x86_64",
    expectedTag: "v2.0.0",
    outputDirectory: output,
  }, { fetchImpl: async () => { calls += 1; throw new Error("unexpected fetch"); } }), /previous-release signing public key/i);
  assert.equal(calls, 0);
});

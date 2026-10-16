import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { gzipSync } from "node:zlib";

import { prepareDraftAssetDirectory, releaseAssetNames } from "../release-assets.mjs";
import {
  assertSameDraftReceipt,
  downloadVerifiedPublicDraft,
  expectedDraftAssetNames,
  validateDraftReleaseIdentity,
} from "../public-draft-assets.mjs";

/** 生成可被 Task5 validator 接受的最小 PE。 */
function pe(marker) {
  const bytes = Buffer.alloc(160, marker);
  bytes.write("MZ", 0); bytes.writeUInt32LE(64, 60); bytes.write("PE\0\0", 64);
  return bytes;
}

/** 生成可被 Task5 validator 接受的最小 DMG。 */
function dmg() {
  const bytes = Buffer.alloc(1024, 3); bytes.write("koly", bytes.length - 512); return bytes;
}

/** 生成可被 Task5 validator 接受的最小 app.tar.gz。 */
function appArchive() {
  const tar = Buffer.alloc(1024); tar.write("Narrant.app/", 0); tar.write("ustar", 257); return gzipSync(tar);
}

/** 把 minisign inner text 包成 canonical outer base64。 */
function outer(lines) {
  const adjusted = [...lines];
  while (Buffer.byteLength(adjusted.join("\n")) % 3 === 0) adjusted[0] += "x";
  return Buffer.from(adjusted.join("\n")).toString("base64");
}

/** 生成与客户端 ED/BLAKE2b minisign 语义相同的 key/signature。 */
function signing(packages) {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const raw = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const id = Buffer.from("0102030405060708", "hex");
  const encodedPublicKey = outer(["untrusted comment: fixture", Buffer.concat([Buffer.from("Ed"), id, raw]).toString("base64")]);
  const signatures = new Map();
  for (const [name, bytes] of packages) {
    const signature = sign(null, createHash("blake2b512").update(bytes).digest(), privateKey);
    const comment = `fixture ${name}`;
    const globalSignature = sign(null, Buffer.concat([signature, Buffer.from(comment)]), privateKey);
    signatures.set(name, outer([
      "untrusted comment: fixture signature",
      Buffer.concat([Buffer.from("ED"), id, signature]).toString("base64"),
      `trusted comment: ${comment}`,
      globalSignature.toString("base64"),
    ]));
  }
  return { publicKey: encodedPublicKey, signatures };
}

/** 准备本地完整八资产。 */
async function assetsFixture(context) {
  const directory = await mkdtemp(join(tmpdir(), "narrant-public-draft-source-"));
  const output = await mkdtemp(join(tmpdir(), "narrant-public-draft-output-"));
  context.after(() => Promise.all([rm(directory, { recursive: true, force: true }), rm(output, { recursive: true, force: true })]));
  const names = releaseAssetNames("2.0.0");
  const packages = new Map([[names.updater.darwin, appArchive()], [names.updater.windows, pe(7)]]);
  const key = signing(packages);
  await Promise.all([
    writeFile(join(directory, names.installer.darwin), dmg()),
    writeFile(join(directory, names.installer.windows), pe(9)),
    ...[...packages].flatMap(([name, bytes]) => [
      writeFile(join(directory, name), bytes), writeFile(join(directory, `${name}.sig`), key.signatures.get(name)),
    ]),
  ]);
  prepareDraftAssetDirectory({
    directory, tag: "v2.0.0", version: "2.0.0", notes: "公开说明",
    releaseSigningPublicKey: key.publicKey,
    candidateUpdaterPublicKey: key.publicKey,
    bridgeTag: "",
    privateRepository: "XLCYun/narrant",
  });
  const assetNames = expectedDraftAssetNames("2.0.0");
  const bytes = new Map(await Promise.all(assetNames.map(async (name) => [name, await readFile(join(directory, name))])));
  const release = {
    id: 42, tag_name: "v2.0.0", draft: true, prerelease: false, body: "公开说明",
    url: "https://api.github.com/repos/XLCYun/narrant/releases/42",
    upload_url: "https://uploads.github.com/repos/XLCYun/narrant/releases/42/assets{?name,label}",
    html_url: "https://github.com/XLCYun/narrant/releases/tag/untagged-testfixture",
    assets: assetNames.map((name, index) => ({
      id: index + 100, name, state: "uploaded", size: bytes.get(name).length,
      url: `https://api.github.com/repos/XLCYun/narrant/releases/assets/${index + 100}`,
      browser_download_url: `https://github.com/XLCYun/narrant/releases/download/untagged-testfixture/${encodeURIComponent(name)}`,
    })),
  };
  return { directory, output, key, bytes, release };
}

/** 构造 API→官方 asset redirect 的 fetch mock，并记录 header。 */
function fetchMock(fixture, mutate = (bytes) => bytes) {
  const calls = [];
  const implementation = async (url, options = {}) => {
    calls.push({ url: String(url), options });
    if (String(url).endsWith("/repos/XLCYun/narrant/releases/42")) {
      return new globalThis.Response(JSON.stringify(fixture.release), { status: 200, headers: { "content-type": "application/json" } });
    }
    const apiAsset = fixture.release.assets.find((asset) => asset.url === String(url));
    if (apiAsset) {
      return new globalThis.Response(null, { status: 302, headers: { location: `https://release-assets.githubusercontent.com/download/${apiAsset.id}` } });
    }
    const id = Number(String(url).split("/").at(-1));
    const asset = fixture.release.assets.find((candidate) => candidate.id === id);
    if (asset) {
      const body = mutate(Buffer.from(fixture.bytes.get(asset.name)), asset);
      return new globalThis.Response(body, { status: 200, headers: { "content-length": String(body.length), "content-type": "application/octet-stream" } });
    }
    throw new Error("unexpected fetch");
  };
  return { implementation, calls };
}

test("固定 Release ID 回读八资产并校验 bytes/checksums/manifest/minisign", async (context) => {
  const fixture = await assetsFixture(context);
  const mock = fetchMock(fixture);
  const result = await downloadVerifiedPublicDraft({
    releaseId: 42, tag: "v2.0.0", version: "2.0.0", outputDirectory: fixture.output,
    token: "secret-token", mode: "normal",
    releaseSigningPublicKey: fixture.key.publicKey,
    candidateUpdaterPublicKey: fixture.key.publicKey, bridgeTag: "",
  }, { fetchImpl: mock.implementation });
  assert.equal(result.assets.length, 8);
  assert.equal(result.updaterTrust.bridgeAuthorized, false);
  assert.equal(result.manifestSha256, createHash("sha256").update(fixture.bytes.get("latest.json")).digest("hex"));
  assert.equal(mock.calls.every((call) => !call.url.includes("secret-token")), true);
  const redirected = mock.calls.filter((call) => call.url.startsWith("https://release-assets.githubusercontent.com/"));
  assert.equal(redirected.every((call) => call.options.headers.Authorization === undefined), true);
  assert.equal(mock.calls.filter((call) => call.url.startsWith("https://api.github.com/")).every((call) => call.options.headers.Authorization === "Bearer secret-token"), true);
});

test("public Draft receipt 绑定 old signing 与 successor key 身份，且 bootstrap 拒绝 bridge", async (context) => {
  const fixture = await assetsFixture(context);
  const successor = signing(new Map([["unused", Buffer.from("successor")]])).publicKey;
  const accepted = await downloadVerifiedPublicDraft({
    releaseId: 42, tag: "v2.0.0", version: "2.0.0", outputDirectory: fixture.output,
    token: "secret-token", mode: "normal",
    releaseSigningPublicKey: fixture.key.publicKey,
    candidateUpdaterPublicKey: successor, bridgeTag: "v2.0.0",
  }, { fetchImpl: fetchMock(fixture).implementation });
  assert.equal(accepted.updaterTrust.bridgeAuthorized, true);
  assert.notEqual(
    accepted.updaterTrust.releaseSigningKeySha256,
    accepted.updaterTrust.candidateUpdaterKeySha256,
  );

  await rm(fixture.output, { recursive: true, force: true });
  await assert.rejects(() => downloadVerifiedPublicDraft({
    releaseId: 42, tag: "v2.0.0", version: "2.0.0", outputDirectory: fixture.output,
    token: "secret-token", mode: "bootstrap",
    releaseSigningPublicKey: fixture.key.publicKey,
    candidateUpdaterPublicKey: successor, bridgeTag: "v2.0.0",
  }, { fetchImpl: fetchMock(fixture).implementation }), /bootstrap.*bridge/i);
});

test("Draft identity 拒绝发布状态、tag/ID 漂移与错误仓库 API URL", () => {
  const release = {
    id: 42, tag_name: "v2.0.0", draft: true, prerelease: false, body: "notes",
    url: "https://api.github.com/repos/XLCYun/narrant/releases/42",
    upload_url: "https://uploads.github.com/repos/XLCYun/narrant/releases/42/assets{?name,label}",
    html_url: "https://github.com/XLCYun/narrant/releases/tag/untagged-testfixture",
  };
  assert.doesNotThrow(() => validateDraftReleaseIdentity(release, { releaseId: 42, tag: "v2.0.0" }));
  for (const mutate of [
    (copy) => { copy.draft = false; }, (copy) => { copy.id = 43; },
    (copy) => { copy.tag_name = "v2.0.1"; }, (copy) => { copy.url = copy.url.replace("XLCYun", "other"); },
    (copy) => { copy.upload_url = copy.upload_url.replace("XLCYun", "other"); },
    (copy) => { copy.html_url = copy.html_url.replace("XLCYun", "other"); },
  ]) {
    const copy = { ...release }; mutate(copy);
    assert.throws(() => validateDraftReleaseIdentity(copy, { releaseId: 42, tag: "v2.0.0" }), /不匹配|已发布/);
  }
  assert.doesNotThrow(() => validateDraftReleaseIdentity(
    { ...release, html_url: "https://github.com/XLCYun/narrant/releases/tag/v2.0.0" },
    { releaseId: 42, tag: "v2.0.0" },
  ));
});

test("Draft asset 下载身份必须绑定 Release 当前 untagged html identity", async (context) => {
  const fixture = await assetsFixture(context);
  fixture.release.assets[0].browser_download_url =
    fixture.release.assets[0].browser_download_url.replace("untagged-testfixture", "v2.0.0");
  await assert.rejects(() => downloadVerifiedPublicDraft({
    releaseId: 42, tag: "v2.0.0", version: "2.0.0", outputDirectory: fixture.output,
    token: "secret-token", mode: "normal",
    releaseSigningPublicKey: fixture.key.publicKey,
    candidateUpdaterPublicKey: fixture.key.publicKey, bridgeTag: "",
  }, { fetchImpl: fetchMock(fixture).implementation }), /identity 无效/);
});

test("asset byte/size 变化 fail closed 且不留下部分文件", async (context) => {
  const fixture = await assetsFixture(context);
  const mock = fetchMock(fixture, (bytes, asset) => asset.name === "latest.json" ? Buffer.concat([bytes, Buffer.from("x")]) : bytes);
  await assert.rejects(() => downloadVerifiedPublicDraft({
    releaseId: 42, tag: "v2.0.0", version: "2.0.0", outputDirectory: fixture.output,
    token: "secret-token", mode: "normal",
    releaseSigningPublicKey: fixture.key.publicKey,
    candidateUpdaterPublicKey: fixture.key.publicKey, bridgeTag: "",
  }, { fetchImpl: mock.implementation }), /Content-Length|字节数/);
});

test("Draft asset redirect 只接受默认 TLS 端口的 GitHub 官方资产域名", async (context) => {
  const fixture = await assetsFixture(context);
  const fallback = fetchMock(fixture);
  const calls = [];
  const implementation = async (url, options) => {
    calls.push(String(url));
    if (String(url).includes("/releases/assets/")) {
      return new globalThis.Response(null, {
        status: 302,
        headers: { location: "https://release-assets.githubusercontent.com:444/untrusted" },
      });
    }
    return fallback.implementation(url, options);
  };
  await assert.rejects(() => downloadVerifiedPublicDraft({
    releaseId: 42, tag: "v2.0.0", version: "2.0.0", outputDirectory: fixture.output,
    token: "secret-token", mode: "normal",
    releaseSigningPublicKey: fixture.key.publicKey,
    candidateUpdaterPublicKey: fixture.key.publicKey, bridgeTag: "",
  }, { fetchImpl: implementation }), /官方资产域名/);
  assert.equal(calls.some((url) => url.includes("secret-token")), false);
  assert.equal(calls.some((url) => url.includes(":444/")), false);
});

test("receipt 比较绑定 asset/key ID/size/SHA，不接受同名不同 bytes", () => {
  const left = {
    repository: "XLCYun/narrant", releaseId: 42, tag: "v2.0.0", version: "2.0.0",
    manifestSha256: "a".repeat(64),
    updaterTrust: {
      releaseSigningKeySha256: "c".repeat(64),
      candidateUpdaterKeySha256: "c".repeat(64),
      bridgeAuthorized: false,
    },
    assets: [{ id: 1, name: "a", size: 1, sha256: "b".repeat(64) }],
  };
  const clone = (value) => JSON.parse(JSON.stringify(value));
  assert.doesNotThrow(() => assertSameDraftReceipt(left, clone(left)));
  const changed = clone(left); changed.assets[0].sha256 = "c".repeat(64);
  assert.throws(() => assertSameDraftReceipt(changed, left), /已变化/);
  const changedKey = clone(left); changedKey.updaterTrust.candidateUpdaterKeySha256 = "d".repeat(64);
  assert.throws(() => assertSameDraftReceipt(changedKey, left), /key identity|已变化/i);
});

import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { generateKeyPairSync } from "node:crypto";
import test from "node:test";

import { expectedDraftAssetNames } from "../public-draft-assets.mjs";
import { assertBootstrapTransition, promoteStableRelease, verifyBootstrapReleaseHistory } from "../promote-stable-release.mjs";
import { updaterPublicKeySha256 } from "../release-assets.mjs";

/** 生成只用于 protected previous-key identity 的 canonical minisign 公钥。 */
function publicKeyFixture(idHex = "0102030405060708") {
  const { publicKey } = generateKeyPairSync("ed25519");
  const raw = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const lines = [
    "untrusted comment: promotion previous key",
    Buffer.concat([Buffer.from("Ed"), Buffer.from(idHex, "hex"), raw]).toString("base64"),
  ];
  while (Buffer.byteLength(lines.join("\n")) % 3 === 0) lines[0] += "x";
  return Buffer.from(lines.join("\n")).toString("base64");
}

const previousPublicKey = publicKeyFixture();

/** 构造固定 Draft receipt 与 API assets。 */
function receipt() {
  return {
    schemaVersion: 1, repository: "cyunlab/narrant", releaseId: 42,
    tag: "v2.0.0", version: "2.0.0", manifestSha256: "a".repeat(64),
    updaterTrust: {
      releaseSigningKeySha256: "d".repeat(64),
      candidateUpdaterKeySha256: "d".repeat(64),
      bridgeAuthorized: false,
    },
    assets: expectedDraftAssetNames("2.0.0").sort().map((name, index) => ({ id: index + 100, name, size: index + 10, sha256: String(index + 1).padStart(64, "0") })),
  };
}

/** 构造 GitHub API Release。 */
function release({ id = 42, tag = "v2.0.0", draft = true, assets = receipt().assets } = {}) {
  return {
    id, tag_name: tag, draft, prerelease: false, body: "notes",
    url: `https://api.github.com/repos/cyunlab/narrant/releases/${id}`,
    upload_url: `https://uploads.github.com/repos/cyunlab/narrant/releases/${id}/assets{?name,label}`,
    html_url: "https://github.com/cyunlab/narrant/releases/tag/untagged-testfixture",
    assets: assets.map(({ id: assetId, name, size }) => ({ id: assetId, name, size, state: "uploaded" })),
  };
}

/** 生成 promotion 固定 options。 */
function options(mode = "normal") {
  return {
    mode,
    sourceTag: "v2.0.0",
    sourceSha: "b".repeat(40),
    publicReleaseId: 42,
    publicTag: "v2.0.0",
    version: "2.0.0",
    previousTag: mode === "normal" ? "v1.9.0" : undefined,
    previousManifestSha256: mode === "normal" ? "c".repeat(64) : undefined,
    bootstrapTag: mode === "bootstrap" ? "v2.0.0" : undefined,
    evidenceDirectory: "/evidence",
    draftDirectory: "/draft",
    maxAgeHours: 24,
    token: "secret-token",
    releaseSigningPublicKey: "old-public-key",
    candidateUpdaterPublicKey: "candidate-public-key",
    bridgeTag: "v2.0.0",
    previousReleaseSigningPublicKey: mode === "normal" ? previousPublicKey : undefined,
  };
}

/** 返回按队列响应的 fetch，并记录是否发生 PATCH。 */
function queuedFetch(responses) {
  const calls = [];
  return {
    calls,
    implementation: async (url, init) => {
      calls.push({ url: String(url), init });
      const response = responses.shift();
      if (response instanceof Error) throw response;
      return new globalThis.Response(JSON.stringify(response.body), {
        status: response.status ?? 200,
        headers: { "content-type": "application/json" },
      });
    },
  };
}

test("normal promotion 在 verify 后只 PATCH fixed Release ID 并回读 Latest", async () => {
  const beforeLatest = release({ id: 12, tag: "v1.9.0", draft: false });
  const draft = release();
  const published = release({ draft: false });
  const fetch = queuedFetch([
    { body: beforeLatest }, { body: draft }, { body: published }, { body: published },
  ]);
  let verified;
  let draftOptions;
  const result = await promoteStableRelease(options(), {
    fetchImpl: fetch.implementation,
    downloadDraft: async (value) => { draftOptions = value; return receipt(); },
    fetchStableManifest: async () => ({ manifest: { version: "1.9.0" }, sha256: "c".repeat(64) }),
    verifyEvidence: async (_directory, expectations) => { verified = expectations; },
  });
  assert.deepEqual(result, { releaseId: 42, tag: "v2.0.0", mode: "normal" });
  assert.equal(verified.draftReceipt.assets.length, 8);
  assert.equal(
    verified.previousReleaseSigningKeySha256,
    updaterPublicKeySha256(previousPublicKey),
  );
  assert.deepEqual({
    mode: draftOptions.mode,
    releaseSigningPublicKey: draftOptions.releaseSigningPublicKey,
    candidateUpdaterPublicKey: draftOptions.candidateUpdaterPublicKey,
    bridgeTag: draftOptions.bridgeTag,
  }, {
    mode: "normal",
    releaseSigningPublicKey: "old-public-key",
    candidateUpdaterPublicKey: "candidate-public-key",
    bridgeTag: "v2.0.0",
  });
  const patches = fetch.calls.filter((call) => call.init.method === "PATCH");
  assert.equal(patches.length, 1);
  assert.equal(patches[0].url, "https://api.github.com/repos/cyunlab/narrant/releases/42");
  assert.deepEqual(JSON.parse(patches[0].init.body), { draft: false, prerelease: false, make_latest: "true" });
  assert.equal(fetch.calls.every((call) => !call.url.includes("secret-token")), true);
  assert.equal(fetch.calls.every((call) => call.init.headers.Authorization === "Bearer secret-token"), true);
});

test("evidence、Stable manifest 或 Draft asset 漂移时 PATCH 永不发生", async () => {
  for (const scenario of ["evidence", "stable", "draft"]) {
    const changedAssets = receipt().assets.map((asset, index) => index === 0 ? { ...asset, size: asset.size + 1 } : asset);
    const fetch = queuedFetch([
      { body: release({ id: 12, tag: "v1.9.0", draft: false }) },
      ...(scenario === "stable" ? [] : [{ body: release({ assets: scenario === "draft" ? changedAssets : receipt().assets }) }]),
    ]);
    await assert.rejects(() => promoteStableRelease(options(), {
      fetchImpl: fetch.implementation,
      downloadDraft: async () => receipt(),
      fetchStableManifest: async () => ({ manifest: { version: "1.9.0" }, sha256: scenario === "stable" ? "d".repeat(64) : "c".repeat(64) }),
      verifyEvidence: async () => { if (scenario === "evidence") throw new Error("stale evidence"); },
    }), scenario === "evidence" ? /stale evidence/ : scenario === "stable" ? /identity 已变化/ : /ID\/size/);
    assert.equal(fetch.calls.some((call) => call.init.method === "PATCH"), false);
  }
});

test("bootstrap protected tag 必须与 candidate 完全匹配", async () => {
  assert.doesNotThrow(() => assertBootstrapTransition("v2.0.0", "v2.0.0"));
  assert.throws(() => assertBootstrapTransition("v2.0.0", "v2.0.1"), /protected/);
});

test("bootstrap promotion 不要求 previous Stable，但 normal evidence 不能混入", async () => {
  const latestLegacy = release({ id: 1, tag: "v1.1.5", draft: false, assets: [] });
  const draft = release();
  const published = release({ draft: false });
  const fetch = queuedFetch([{ body: [draft, latestLegacy] }, { body: draft }, { body: published }, { body: published }]);
  let mode;
  await promoteStableRelease(options("bootstrap"), {
    fetchImpl: fetch.implementation,
    downloadDraft: async () => receipt(),
    verifyEvidence: async (_directory, expectations) => { mode = expectations.mode; },
  });
  assert.equal(mode, "bootstrap");
});

test("bootstrap promotion 拒绝 protected Environment 中的 previous release key", async () => {
  const bootstrap = options("bootstrap");
  bootstrap.previousReleaseSigningPublicKey = previousPublicKey;
  let downloaded = false;
  await assert.rejects(() => promoteStableRelease(bootstrap, {
    downloadDraft: async () => { downloaded = true; return receipt(); },
  }), /bootstrap.*previous release key/i);
  assert.equal(downloaded, false);
});

test("历史 published updater Stable 会永久关闭 bootstrap，即使当前 Latest 是 legacy", async () => {
  const draft = release();
  const historical = release({ id: 9, tag: "v1.9.0", draft: false, assets: [{ id: 91, name: "latest.json", size: 10 }] });
  const legacyLatest = release({ id: 1, tag: "v1.1.5", draft: false, assets: [] });
  const fetch = queuedFetch([{ body: [draft, historical, legacyLatest] }]);
  await assert.rejects(() => promoteStableRelease(options("bootstrap"), {
    fetchImpl: fetch.implementation,
    downloadDraft: async () => receipt(),
    verifyEvidence: async () => {},
  }), /永久关闭/);
  assert.equal(fetch.calls.some((call) => call.init.method === "PATCH"), false);
  assert.match(fetch.calls[0].url, /releases\?per_page=100&page=1$/);
});

test("bootstrap history scan 有界分页并在后续 API 错误时 fail closed", async () => {
  const fullPage = Array.from({ length: 100 }, (_value, index) =>
    release({ id: index + 1000, tag: `v1.0.${index}`, draft: false, assets: [] }));
  const paged = queuedFetch([{ body: fullPage }, { body: [] }]);
  await verifyBootstrapReleaseHistory({
    token: "secret-token", candidateReleaseId: 42, candidateTag: "v2.0.0",
  }, { fetchImpl: paged.implementation });
  assert.deepEqual(paged.calls.map(({ url }) => url), [
    "https://api.github.com/repos/cyunlab/narrant/releases?per_page=100&page=1",
    "https://api.github.com/repos/cyunlab/narrant/releases?per_page=100&page=2",
  ]);

  const error = queuedFetch([{ body: fullPage }, new Error("page two unavailable")]);
  await assert.rejects(() => verifyBootstrapReleaseHistory({
    token: "secret-token", candidateReleaseId: 42, candidateTag: "v2.0.0",
  }, { fetchImpl: error.implementation }), /网络请求失败/);

  const bounded = queuedFetch(Array.from({ length: 20 }, (_value, page) => ({
    body: fullPage.map((item, index) => ({ ...item, id: 10_000 + page * 100 + index })),
  })));
  await assert.rejects(() => verifyBootstrapReleaseHistory({
    token: "secret-token", candidateReleaseId: 42, candidateTag: "v2.0.0",
  }, { fetchImpl: bounded.implementation }), /分页上限/);
  assert.equal(bounded.calls.length, 20);
});

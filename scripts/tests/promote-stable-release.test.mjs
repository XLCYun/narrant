import assert from "node:assert/strict";
import test from "node:test";

import { expectedDraftAssetNames } from "../public-draft-assets.mjs";
import { assertBootstrapTransition, promoteStableRelease } from "../promote-stable-release.mjs";

/** 构造固定 Draft receipt 与 API assets。 */
function receipt() {
  return {
    schemaVersion: 1, repository: "cyunlab/narrant", releaseId: 42,
    tag: "v2.0.0", version: "2.0.0", manifestSha256: "a".repeat(64),
    assets: expectedDraftAssetNames("2.0.0").sort().map((name, index) => ({ id: index + 100, name, size: index + 10, sha256: String(index + 1).padStart(64, "0") })),
  };
}

/** 构造 GitHub API Release。 */
function release({ id = 42, tag = "v2.0.0", draft = true, assets = receipt().assets } = {}) {
  return {
    id, tag_name: tag, draft, prerelease: false, body: "notes",
    url: `https://api.github.com/repos/cyunlab/narrant/releases/${id}`,
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
    publicKey: "public-key",
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
  const result = await promoteStableRelease(options(), {
    fetchImpl: fetch.implementation,
    downloadDraft: async () => receipt(),
    fetchStableManifest: async () => ({ manifest: { version: "1.9.0" }, sha256: "c".repeat(64) }),
    verifyEvidence: async (_directory, expectations) => { verified = expectations; },
  });
  assert.deepEqual(result, { releaseId: 42, tag: "v2.0.0", mode: "normal" });
  assert.equal(verified.draftReceipt.assets.length, 8);
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

test("bootstrap 仅在 protected tag 匹配且当前 Latest 无 latest.json 时开放", async () => {
  assert.doesNotThrow(() => assertBootstrapTransition(release({ id: 1, tag: "v1.1.5", draft: false, assets: [] }), "v2.0.0", "v2.0.0"));
  assert.throws(() => assertBootstrapTransition(release({ assets: [{ id: 1, name: "latest.json", size: 1 }] }), "v2.0.0", "v2.0.0"), /永久关闭/);
  assert.throws(() => assertBootstrapTransition(undefined, "v2.0.0", "v2.0.1"), /protected/);
});

test("bootstrap promotion 不要求 previous Stable，但 normal evidence 不能混入", async () => {
  const latestLegacy = release({ id: 1, tag: "v1.1.5", draft: false, assets: [] });
  const draft = release();
  const published = release({ draft: false });
  const fetch = queuedFetch([{ body: latestLegacy }, { body: draft }, { body: published }, { body: published }]);
  let mode;
  await promoteStableRelease(options("bootstrap"), {
    fetchImpl: fetch.implementation,
    downloadDraft: async () => receipt(),
    verifyEvidence: async (_directory, expectations) => { mode = expectations.mode; },
  });
  assert.equal(mode, "bootstrap");
});

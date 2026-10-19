import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import test from "node:test";

import { readPublicStableIdentity } from "../release/read-public-stable-identity.mjs";

const bytes = Buffer.from(JSON.stringify({
  version: "1.2.13",
  platforms: {
    "darwin-aarch64": {
      url: "https://github.com/XLCYun/narrant/releases/download/v1.2.13/narrant-darwin-aarch64-updater.app.tar.gz",
      signature: "signature-a",
    },
    "windows-x86_64": {
      url: "https://github.com/XLCYun/narrant/releases/download/v1.2.13/narrant-windows-x86_64-updater.exe",
      signature: "signature-b",
    },
  },
}));

/** 构造最终落在 GitHub release asset 链路的匿名响应。 */
function response(body = bytes) {
  const value = new globalThis.Response(body, { status: 200, headers: { "content-type": "application/octet-stream" } });
  Object.defineProperty(value, "url", { value: "https://release-assets.githubusercontent.com/object" });
  return value;
}

test("固定匿名 Stable manifest 输出 tag 与 exact bytes SHA-256", async () => {
  const result = await readPublicStableIdentity({}, { fetchImpl: async () => response() });
  assert.deepEqual(result, {
    tag: "v1.2.13",
    manifestSha256: createHash("sha256").update(bytes).digest("hex"),
  });
});

test("Stable identity 拒绝非两目标和跨 tag package URL", async () => {
  for (const mutate of [
    (manifest) => { delete manifest.platforms["windows-x86_64"]; },
    (manifest) => { manifest.platforms["darwin-aarch64"].url = manifest.platforms["darwin-aarch64"].url.replace("v1.2.13", "v9.9.9"); },
  ]) {
    const manifest = JSON.parse(bytes);
    mutate(manifest);
    await assert.rejects(
      readPublicStableIdentity({}, { fetchImpl: async () => response(Buffer.from(JSON.stringify(manifest))) }),
      /Stable manifest/,
    );
  }
});

import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import test from "node:test";

import { readPublicStableIdentity } from "../read-public-stable-identity.mjs";

const bytes = Buffer.from(JSON.stringify({
  version: "1.2.13",
  platforms: {
    "darwin-aarch64": {
      url: "https://updates.cyunlab.com/narrant/releases/1.2.13/darwin-aarch64/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-narrant-darwin-aarch64-updater.app.tar.gz",
      installer_url: "https://updates.cyunlab.com/narrant/releases/1.2.13/darwin-aarch64/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-Narrant_1.2.13_aarch64.dmg",
      signature: "signature-a",
    },
    "windows-x86_64": {
      url: "https://updates.cyunlab.com/narrant/releases/1.2.13/windows-x86_64/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc-narrant-windows-x86_64-updater.exe",
      installer_url: "https://updates.cyunlab.com/narrant/releases/1.2.13/windows-x86_64/dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd-Narrant_1.2.13_x64-setup.exe",
      signature: "signature-b",
    },
  },
}));

/** 构造最终落在 OSS Stable 对象的匿名响应。 */
function response(body = bytes) {
  const value = new globalThis.Response(body, { status: 200, headers: { "content-type": "application/octet-stream" } });
  Object.defineProperty(value, "url", { value: "https://updates.cyunlab.com/narrant/channels/stable/latest.json" });
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
    (manifest) => { manifest.platforms["darwin-aarch64"].url = manifest.platforms["darwin-aarch64"].url.replace("1.2.13", "9.9.9"); },
  ]) {
    const manifest = JSON.parse(bytes);
    mutate(manifest);
    await assert.rejects(
      readPublicStableIdentity({}, { fetchImpl: async () => response(Buffer.from(JSON.stringify(manifest))) }),
      /Stable manifest/,
    );
  }
});

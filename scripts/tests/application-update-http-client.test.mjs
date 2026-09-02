import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { URL } from "node:url";

const cargoPath = new URL("../../src-tauri/Cargo.toml", import.meta.url);
const metadataPath = new URL("../../src-tauri/src/application_update/metadata.rs", import.meta.url);
const runtimePath = new URL("../../src-tauri/src/application_update/runtime.rs", import.meta.url);

test("production reqwest 使用 Rustls native roots 且保留 manifest/package redirect allowlist", async () => {
  const [cargo, metadata, runtime] = await Promise.all([
    readFile(cargoPath, "utf8"),
    readFile(metadataPath, "utf8"),
    readFile(runtimePath, "utf8"),
  ]);
  const reqwest = cargo.match(/^reqwest\s*=\s*\{[^\n]+\}$/m)?.[0] ?? "";
  assert.match(reqwest, /features\s*=\s*\[[^\]]*"rustls-tls-native-roots"/);
  assert.doesNotMatch(reqwest, /"rustls-tls"(?:\s*[,\]])/);
  assert.doesNotMatch(reqwest, /webpki/);
  assert.match(metadata, /redirect\(reqwest::redirect::Policy::custom/);
  assert.match(metadata, /trusted_manifest_url\(candidate\)/);
  assert.match(runtime, /redirect\(reqwest::redirect::Policy::custom/);
  assert.match(runtime, /trusted_redirect_url\(attempt\.url\(\)\)/);
});

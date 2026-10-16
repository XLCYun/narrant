import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import test from "node:test";

import { normalizeGeneratedPublicKey } from "../build-update-test-packages.mjs";
import { normalizeTauriUpdaterSignature, verifyTauriUpdaterSignature } from "../release-assets.mjs";
import { runBoundedCommand, selectCommandEnvironment } from "../update-smoke-tools.mjs";
import { verifyWindowsVersionResource } from "../verify-production-update-artifacts.mjs";

test("实际 Tauri signer 的测试公钥和签名可被正式格式校验器验证", async t => {
  const directory = await mkdtemp(path.join(tmpdir(), "narrant-test-signature-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const cli = path.resolve(import.meta.dirname, "../../node_modules/@tauri-apps/cli/tauri.js");
  const key = path.join(directory, "test.key");
  const packagePath = path.join(directory, "fixture.bin");
  const environment = selectCommandEnvironment(process.env);
  await runBoundedCommand(process.execPath, [cli, "signer", "generate", "--ci", "--password", "fixture", "--write-keys", key], { environment });
  const publicKey = normalizeGeneratedPublicKey(await readFile(`${key}.pub`, "utf8"));
  assert.equal(normalizeGeneratedPublicKey(publicKey), publicKey);
  await writeFile(packagePath, "isolated test package");
  await runBoundedCommand(process.execPath, [cli, "signer", "sign", "--private-key-path", key, "--password", "fixture", packagePath], { environment });
  const signature = normalizeTauriUpdaterSignature(await readFile(`${packagePath}.sig`, "utf8"));
  assert.doesNotThrow(() => verifyTauriUpdaterSignature("isolated test package", signature, publicKey));
  assert.throws(() => verifyTauriUpdaterSignature("changed package", signature, publicKey));
});

test("正式 Windows 包必须报告正确产品与版本，不能靠重命名旧包通过", () => {
  for (const productVersion of ["1.2.15", "1.2.15.0"]) {
    assert.doesNotThrow(() => verifyWindowsVersionResource({ productName: "Narrant", productVersion }, "1.2.15"));
  }
  for (const resource of [
    { productName: "Narrant", productVersion: "1.2.14" },
    { productName: "Narrant Upgrade Test", productVersion: "1.2.15" },
    { productName: "Narrant", productVersion: "1.2.15.1" },
    {},
  ]) assert.throws(() => verifyWindowsVersionResource(resource, "1.2.15"), /身份或版本/);
});

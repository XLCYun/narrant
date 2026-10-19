import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import test from "node:test";

import {
  buildTauriArguments,
  buildTauriInvocation,
  normalizeUpdaterPublicKey,
} from "../dev/run-tauri.mjs";

const PUBLIC_KEY = Buffer.from(
  "untrusted comment: minisign public key: DFDAA0F6324E5A1\nRWSh5SRjD6r9DemB3l4PakKrUZY75SJqG+AZ55Tl03px1CbAMYYNsAzS",
).toString("base64");

test("正式 build 把同一 updater 公钥合并进 Tauri config", () => {
  const args = buildTauriArguments("build", ["--target", "aarch64-apple-darwin"], {
    NARRANT_UPDATER_PUBLIC_KEY: PUBLIC_KEY,
  });
  assert.deepEqual(args.slice(0, 2), ["build", "--config"]);
  assert.equal(JSON.parse(args[2]).plugins.updater.pubkey, PUBLIC_KEY);
  assert.deepEqual(args.slice(3), ["--target", "aarch64-apple-darwin"]);
});

test("Windows 通过 Node 与 pnpm argv 保留完整 updater JSON，不经过 shell", () => {
  const invocation = buildTauriInvocation("build", ["--bundles", "nsis"], {
    NARRANT_UPDATER_PUBLIC_KEY: PUBLIC_KEY,
    npm_execpath: "C:\\pnpm\\pnpm.cjs",
  }, "C:\\node\\node.exe");
  assert.equal(invocation.executable, "C:\\node\\node.exe");
  assert.deepEqual(invocation.args.slice(0, 5), [
    "C:\\pnpm\\pnpm.cjs",
    "exec",
    "tauri",
    "build",
    "--config",
  ]);
  assert.equal(JSON.parse(invocation.args[5]).plugins.updater.pubkey, PUBLIC_KEY);
  assert.deepEqual(invocation.args.slice(6), ["--bundles", "nsis"]);
});

test("非 build 命令和无公钥的本地 build 不注入配置", () => {
  assert.deepEqual(buildTauriArguments("signer", ["sign", "asset"], {
    NARRANT_UPDATER_PUBLIC_KEY: PUBLIC_KEY,
  }), ["signer", "sign", "asset"]);
  assert.deepEqual(buildTauriArguments("build", ["--", "--debug"], {}), ["build", "--debug"]);
});

test("公钥必须是 canonical base64 minisign 两行文本", () => {
  assert.equal(normalizeUpdaterPublicKey(`  ${PUBLIC_KEY}\n`), PUBLIC_KEY);
  const fileKey = Buffer.from(`${Buffer.from(PUBLIC_KEY, "base64").toString("utf8")}\n`).toString("base64");
  assert.throws(() => normalizeUpdaterPublicKey(fileKey), /NARRANT_UPDATER_PUBLIC_KEY/);
  assert.throws(() => normalizeUpdaterPublicKey("not-a-key"), /NARRANT_UPDATER_PUBLIC_KEY/);
  assert.throws(
    () => normalizeUpdaterPublicKey(Buffer.from(`${Buffer.from(PUBLIC_KEY, "base64")}\n\n`).toString("base64")),
    /NARRANT_UPDATER_PUBLIC_KEY/,
  );
  assert.throws(
    () => normalizeUpdaterPublicKey(Buffer.from("untrusted comment: wrong\nRWabc").toString("base64")),
    /NARRANT_UPDATER_PUBLIC_KEY/,
  );
});

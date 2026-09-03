import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { createOssutilStorage, finalizeNarrantOssCandidate, prepareNarrantOssCandidate } from "../narrant-oss-stable.mjs";
import { releaseAssetNames } from "../release-assets.mjs";

/** 创建与公开 Draft 同形的最小 OSS candidate 资产。 */
async function fixture(context) {
  const directory = await mkdtemp(path.join(tmpdir(), "narrant-oss-candidate-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const version = "2.3.4";
  const names = releaseAssetNames(version);
  const bytes = new Map([
    [names.installer.darwin, Buffer.from("dmg")],
    [names.installer.windows, Buffer.from("setup")],
    [names.updater.darwin, Buffer.from("archive")],
    [`${names.updater.darwin}.sig`, Buffer.from("darwin-signature")],
    [names.updater.windows, Buffer.from("updater")],
    [`${names.updater.windows}.sig`, Buffer.from("windows-signature")],
    [names.checksums, Buffer.from("checksums")],
  ]);
  const url = (target, name) => {
    const digest = createHash("sha256").update(bytes.get(name)).digest("hex");
    return `https://updates.cyunlab.com/narrant/releases/${version}/${target}/${digest}-${name}`;
  };
  const manifest = {
    version,
    notes: "Release notes",
    platforms: {
      "darwin-aarch64": {
        url: url("darwin-aarch64", names.updater.darwin),
        installer_url: url("darwin-aarch64", names.installer.darwin),
        signature: "darwin-signature",
      },
      "windows-x86_64": {
        url: url("windows-x86_64", names.updater.windows),
        installer_url: url("windows-x86_64", names.installer.windows),
        signature: "windows-signature",
      },
    },
  };
  bytes.set(names.manifest, Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`));
  for (const [name, body] of bytes) await writeFile(path.join(directory, name), body);
  return { directory, version, manifest };
}

/** 创建可观察不可变写入、锁和 Stable 替换的内存存储。 */
function storage() {
  const values = new Map();
  const events = [];
  const writes = [];
  return {
    values,
    events,
    writes,
    async ensureObject(key, body, metadata) {
      events.push(`ensure:${key}`);
      values.set(key, Buffer.from(body));
      writes.push({ key, metadata });
    },
    async readObject(key) {
      if (!values.has(key)) throw new Error("NoSuchKey");
      return Buffer.from(values.get(key));
    },
    async replaceObject(key, body) { events.push(`replace:${key}`); values.set(key, Buffer.from(body)); },
    async acquirePromotionLock(key, body) { events.push(`lock:${key}`); values.set(key, Buffer.from(body)); },
    async releasePromotionLock(key) { events.push(`unlock:${key}`); },
  };
}

test("Narrant OSS bootstrap 先发布不可变对象，再以锁保护 Stable pointer", async (context) => {
  const source = await fixture(context);
  const target = storage();
  const candidate = await prepareNarrantOssCandidate({
    directory: source.directory,
    version: source.version,
    tag: `v${source.version}`,
    candidateCommit: "0123456789abcdef0123456789abcdef01234567",
    bootstrapTag: `v${source.version}`,
    mode: "bootstrap",
  }, target);
  assert.equal(target.events.some((event) => event.startsWith("replace:")), false);
  assert.equal(target.events.filter((event) => event.startsWith("ensure:")).length, 8);
  assert.deepEqual(
    target.writes.filter((write) => write.metadata.downloadFilename).map((write) => write.metadata.downloadFilename),
    ["Narrant-2.3.4-macOS-arm64.dmg", "Narrant-2.3.4-Windows-x64.exe"],
  );

  const manifest = await finalizeNarrantOssCandidate(candidate, target, {
    lockOwner: "test-run:1",
  });
  assert.deepEqual(manifest, source.manifest);
  assert.deepEqual(target.events.slice(-3).map((event) => event.split(":", 1)[0]), ["lock", "replace", "unlock"]);
});

test("OSS 上传为人工安装包设置安全的 Content-Disposition 文件名", async () => {
  const calls = [];
  const target = createOssutilStorage({
    bucket: "example-bucket",
    region: "cn-shenzhen",
    prefix: "narrant",
    credentials: {
      accessKeyId: "temporary-id",
      accessKeySecret: "temporary-secret",
      securityToken: "temporary-token",
    },
    async runOssutil(args) {
      calls.push(args);
      if (args[1]?.startsWith("oss://")) throw new Error("NoSuchKey: object does not exist");
      return { stdout: Buffer.alloc(0), stderr: "" };
    },
  });

  await target.ensureObject("narrant/releases/2.3.4/darwin-aarch64/package.dmg", Buffer.from("dmg"), {
    cacheControl: "public, max-age=31536000, immutable",
    contentType: "application/octet-stream",
    downloadFilename: "Narrant-2.3.4-macOS-arm64.dmg",
  });
  assert.ok(calls[1].includes("Cache-Control:public, max-age=31536000, immutable#Content-Type:application/octet-stream#Content-Disposition:attachment; filename=\"Narrant-2.3.4-macOS-arm64.dmg\""));

  await assert.rejects(() => target.ensureObject(
    "narrant/releases/2.3.4/darwin-aarch64/unsafe.dmg",
    Buffer.from("dmg"),
    { cacheControl: "no-cache", downloadFilename: "unsafe.dmg\r\nX-Test: injected" },
  ), /safe ASCII basename/);
});

test("Narrant OSS 非 bootstrap 缺少 previous Stable 时失败", async (context) => {
  const source = await fixture(context);
  await assert.rejects(() => prepareNarrantOssCandidate({
    directory: source.directory,
    version: source.version,
    tag: `v${source.version}`,
    candidateCommit: "0123456789abcdef0123456789abcdef01234567",
    bootstrapTag: "v9.9.9",
    mode: "normal",
    previousTag: "v2.3.3",
    previousManifestSha256: "a".repeat(64),
  }, storage()), /NoSuchKey/);
});

test("Narrant OSS bootstrap 在 Stable 已存在时永久拒绝", async (context) => {
  const source = await fixture(context);
  const target = storage();
  target.values.set("narrant/channels/stable/latest.json", Buffer.from('{"version":"2.3.3"}'));
  await assert.rejects(() => prepareNarrantOssCandidate({
    directory: source.directory,
    version: source.version,
    tag: `v${source.version}`,
    candidateCommit: "0123456789abcdef0123456789abcdef01234567",
    bootstrapTag: `v${source.version}`,
    mode: "bootstrap",
  }, target), /已存在/);
});

test("Narrant OSS normal 必须匹配 smoke evidence 的 previous Stable identity", async (context) => {
  const source = await fixture(context);
  const target = storage();
  target.values.set("narrant/channels/stable/latest.json", Buffer.from('{"version":"2.3.3"}'));
  await assert.rejects(() => prepareNarrantOssCandidate({
    directory: source.directory,
    version: source.version,
    tag: `v${source.version}`,
    candidateCommit: "0123456789abcdef0123456789abcdef01234567",
    mode: "normal",
    previousTag: "v2.3.3",
    previousManifestSha256: "a".repeat(64),
  }, target), /smoke evidence identity/);
});

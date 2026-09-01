import { createHash } from "node:crypto";
import { Buffer } from "node:buffer";
import { createWriteStream } from "node:fs";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { pathToFileURL, URL } from "node:url";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";

import { verifyTauriUpdaterSignature } from "./release-assets.mjs";
import { FIXED_STABLE_ENDPOINT } from "./public-draft-assets.mjs";

const MAX_MANIFEST_BYTES = 512 * 1024;
const MAX_PACKAGE_BYTES = 2 * 1024 * 1024 * 1024;

/** 确认 anonymous response 最终仍在 GitHub 官方 release asset 链路。 */
function validateFinalUrl(urlText, kind) {
  let url;
  try { url = new URL(urlText); } catch { throw new Error(`${kind} 最终 URL 无效`); }
  if (
    url.protocol !== "https:" || url.username || url.password || url.port ||
    !["github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"].includes(url.hostname)
  ) {
    throw new Error(`${kind} 最终 URL 离开 GitHub 官方链路`);
  }
}

/** 匿名 GET 一个有界小文件。 */
async function fetchSmall(url, maximum, label, fetchImpl) {
  let response;
  try { response = await fetchImpl(url, { redirect: "follow", headers: { Accept: "application/octet-stream" } }); }
  catch { throw new Error(`${label} 匿名下载失败`); }
  if (!response || response.status < 200 || response.status >= 300) throw new Error(`${label} 返回 HTTP ${response?.status ?? "unknown"}`);
  validateFinalUrl(response.url, label);
  const length = response.headers.get("content-length");
  if (length !== null && (!/^\d+$/.test(length) || Number(length) > maximum)) throw new Error(`${label} 超过上限`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (bytes.length <= 0 || bytes.length > maximum) throw new Error(`${label} 大小无效`);
  return bytes;
}

/** 匿名流式下载 previous package，同时计算完整 SHA-256。 */
async function fetchPackage(url, destination, fetchImpl) {
  let response;
  try { response = await fetchImpl(url, { redirect: "follow", headers: { Accept: "application/octet-stream" } }); }
  catch { throw new Error("previous Stable package 匿名下载失败"); }
  if (!response || response.status < 200 || response.status >= 300 || !response.body) {
    throw new Error(`previous Stable package 返回 HTTP ${response?.status ?? "unknown"}`);
  }
  validateFinalUrl(response.url, "previous Stable package");
  const hash = createHash("sha256");
  let size = 0;
  const verifier = new Transform({
    /** 逐块限制 previous package 总量并计算最终 SHA-256。 */
    transform(chunk, _encoding, callback) {
      size += chunk.length;
      if (size > MAX_PACKAGE_BYTES) {
        callback(new Error("previous Stable package 超过上限"));
        return;
      }
      hash.update(chunk);
      callback(null, chunk);
    },
  });
  try {
    await pipeline(
      Readable.fromWeb(response.body),
      verifier,
      createWriteStream(destination, { flags: "wx", mode: 0o600 }),
    );
    if (size <= 0) throw new Error("previous Stable package 为空");
  } catch (error) {
    await rm(destination, { force: true }).catch(() => {});
    throw error;
  }
  return { size, sha256: hash.digest("hex") };
}

/** 下载并验证当前 published Stable 的单 target baseline。 */
export async function preparePreviousStable(options, dependencies = {}) {
  const fetchImpl = dependencies.fetchImpl ?? globalThis.fetch;
  const { target, expectedTag, outputDirectory, releaseSigningPublicKey } = options;
  if (!["darwin-aarch64", "windows-x86_64"].includes(target)) throw new Error("previous Stable target 无效");
  if (!/^v\d+\.\d+\.\d+$/.test(expectedTag ?? "")) throw new Error("previous Stable tag 无效");
  await mkdir(outputDirectory, { recursive: true });
  const manifestBytes = await fetchSmall(FIXED_STABLE_ENDPOINT, MAX_MANIFEST_BYTES, "published Stable manifest", fetchImpl);
  let manifest;
  try { manifest = JSON.parse(manifestBytes.toString("utf8")); } catch { throw new Error("published Stable manifest 不是 JSON"); }
  const version = expectedTag.slice(1);
  if (manifest.version !== version || Object.keys(manifest.platforms ?? {}).sort().join(",") !== "darwin-aarch64,windows-x86_64") {
    throw new Error("published Stable manifest version/targets 不匹配");
  }
  const entry = manifest.platforms[target];
  const expectedPrefix = `https://github.com/cyunlab/narrant/releases/download/${expectedTag}/`;
  if (typeof entry?.url !== "string" || !entry.url.startsWith(expectedPrefix) || typeof entry.signature !== "string" || !entry.signature) {
    throw new Error("published Stable target entry 无效");
  }
  const parsed = new URL(entry.url);
  if (parsed.href !== entry.url || parsed.search || parsed.hash || parsed.username || parsed.password || parsed.port) {
    throw new Error("published Stable package URL 非 canonical");
  }
  const manifestPath = path.join(outputDirectory, "previous-latest.json");
  const packageBasename = path.posix.basename(parsed.pathname);
  if (!packageBasename || packageBasename === "." || packageBasename === "..") throw new Error("previous Stable package basename 无效");
  const packagePath = path.join(outputDirectory, packageBasename);
  const signaturePath = `${packagePath}.sig`;
  await Promise.all([
    writeFile(manifestPath, manifestBytes, { flag: "wx", mode: 0o600 }),
    writeFile(signaturePath, entry.signature, { flag: "wx", mode: 0o600 }),
  ]);
  const packageIdentity = await fetchPackage(entry.url, packagePath, fetchImpl);
  verifyTauriUpdaterSignature(await readFile(packagePath), entry.signature, releaseSigningPublicKey);
  const result = Object.freeze({
    tag: expectedTag,
    version,
    target,
    manifestSha256: createHash("sha256").update(manifestBytes).digest("hex"),
    packageSha256: packageIdentity.sha256,
    signatureSha256: createHash("sha256").update(entry.signature).digest("hex"),
    packageUrl: entry.url,
    manifestPath,
    packagePath,
    signaturePath,
  });
  await writeFile(path.join(outputDirectory, "previous.json"), `${JSON.stringify(result, null, 2)}\n`, { flag: "wx", mode: 0o600 });
  return result;
}

/** workflow CLI。 */
async function main() {
  const [target, expectedTag, outputDirectory] = process.argv.slice(2);
  const result = await preparePreviousStable({
    target, expectedTag, outputDirectory,
    releaseSigningPublicKey: process.env.NARRANT_RELEASE_SIGNING_PUBLIC_KEY,
  });
  process.stdout.write(`已验证 previous Stable ${result.tag} / ${target}。\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`错误：${error instanceof Error ? error.message : "previous Stable 准备失败"}\n`);
    process.exitCode = 1;
  });
}

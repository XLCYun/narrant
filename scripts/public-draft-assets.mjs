import { createHash } from "node:crypto";
import { Buffer } from "node:buffer";
import { createWriteStream } from "node:fs";
import { mkdir, readFile, rm } from "node:fs/promises";
import { join } from "node:path";
import process from "node:process";
import { pathToFileURL, URL } from "node:url";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import { TextDecoder } from "node:util";

import {
  assertExactNameSet,
  parseReleaseIdentity,
  releaseAssetNames,
  resolveUpdaterKeyPolicy,
  validateUpdaterManifest,
  verifyTauriUpdaterSignature,
} from "./release-assets.mjs";

export const PUBLIC_RELEASE_REPOSITORY = "cyunlab/narrant";
export const FIXED_STABLE_ENDPOINT =
  "https://updates.cyunlab.com/narrant/channels/stable/latest.json";
const API_ORIGIN = "https://api.github.com";
const MAX_JSON_BYTES = 512 * 1024;
const MAX_ASSET_BYTES = 2 * 1024 * 1024 * 1024;

/** 返回一个版本完整公开 Draft 必须拥有的八个名称。 */
export function expectedDraftAssetNames(version) {
  const names = releaseAssetNames(version);
  return [
    names.installer.darwin,
    names.installer.windows,
    names.updater.darwin,
    `${names.updater.darwin}.sig`,
    names.updater.windows,
    `${names.updater.windows}.sig`,
    names.manifest,
    names.checksums,
  ];
}

/** 只接受正整数 GitHub database ID。 */
function requireDatabaseId(value, label) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${label} 必须是正整数`);
  }
  return value;
}

/** 限制读取 GitHub JSON 响应，避免错误页或代理响应耗尽 runner。 */
async function readBoundedResponse(response, maximum, label) {
  const length = response.headers.get("content-length");
  if (length !== null && (!/^\d+$/.test(length) || Number(length) > maximum)) {
    throw new Error(`${label} 响应超过安全上限`);
  }
  if (!response.body || typeof response.body.getReader !== "function") {
    throw new Error(`${label} 缺少可读取响应体`);
  }
  const reader = response.body.getReader();
  const chunks = [];
  let total = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > maximum) throw new Error(`${label} 响应超过安全上限`);
      chunks.push(Buffer.from(value));
    }
  } catch (error) {
    try { await reader.cancel(); } catch { /* 主读取错误优先。 */ }
    throw error;
  } finally {
    try { reader.releaseLock(); } catch { /* 已完整读取，释放失败不改变 bytes。 */ }
  }
  return Buffer.concat(chunks);
}

/** 用 header 携带 token 调 GitHub API；错误中不回显 URL、header 或响应正文。 */
async function githubApiJson(pathname, token, fetchImpl) {
  let response;
  try {
    response = await fetchImpl(`${API_ORIGIN}${pathname}`, {
      redirect: "error",
      headers: {
        Accept: "application/vnd.github+json",
        Authorization: `Bearer ${token}`,
        "X-GitHub-Api-Version": "2022-11-28",
      },
    });
  } catch {
    throw new Error("GitHub Release API 请求失败");
  }
  if (!response || response.redirected || response.status < 200 || response.status >= 300) {
    try { await response?.body?.cancel(); } catch { /* 响应身份错误优先。 */ }
    throw new Error(`GitHub Release API 返回 HTTP ${response?.status ?? "unknown"}`);
  }
  const contentType = response.headers.get("content-type") ?? "";
  if (!/^application\/json(?:\s*;|$)/i.test(contentType)) {
    try { await response.body?.cancel(); } catch { /* Content-Type 错误优先。 */ }
    throw new Error("GitHub Release API 返回错误 Content-Type");
  }
  const bytes = await readBoundedResponse(response, MAX_JSON_BYTES, "GitHub Release API");
  try {
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch {
    throw new Error("GitHub Release API 返回无效 JSON");
  }
}

/** 校验 API Release 始终是固定 public repository 中同一个 Draft。 */
export function validateDraftReleaseIdentity(release, options) {
  requireDatabaseId(options.releaseId, "public Release ID");
  const expectedApiUrl =
    `${API_ORIGIN}/repos/${PUBLIC_RELEASE_REPOSITORY}/releases/${options.releaseId}`;
  const expectedUploadUrl =
    `https://uploads.github.com/repos/${PUBLIC_RELEASE_REPOSITORY}/releases/${options.releaseId}/assets{?name,label}`;
  const htmlPrefix = `https://github.com/${PUBLIC_RELEASE_REPOSITORY}/releases/tag/`;
  if (
    !release ||
    release.id !== options.releaseId ||
    release.tag_name !== options.tag ||
    release.draft !== true ||
    release.prerelease !== false ||
    release.url !== expectedApiUrl ||
    release.upload_url !== expectedUploadUrl ||
    typeof release.html_url !== "string" ||
    !release.html_url.startsWith(htmlPrefix)
  ) {
    throw new Error("public Release ID/tag 不匹配或 Release 已发布");
  }
  const draftIdentity = release.html_url.slice(htmlPrefix.length);
  if (!/^untagged-[A-Za-z0-9]+$/.test(draftIdentity)) {
    throw new Error("public Draft 返回无效 untagged 身份");
  }
  if (typeof release.body !== "string" || !release.body.trim()) {
    throw new Error("公开 Draft Release Notes 不能为空");
  }
  return release;
}

/** 从已验证 Draft 的 html identity 构造该 Release 当前下载根路径。 */
function draftDownloadBaseUrl(release) {
  const htmlPrefix = `https://github.com/${PUBLIC_RELEASE_REPOSITORY}/releases/tag/`;
  const draftIdentity = release.html_url.slice(htmlPrefix.length);
  return `https://github.com/${PUBLIC_RELEASE_REPOSITORY}/releases/download/${draftIdentity}`;
}

/** 校验单个 Draft asset 的固定 API/download identity。 */
function validateAssetIdentity(asset, downloadBaseUrl, expectedName) {
  requireDatabaseId(asset?.id, `Draft asset ${expectedName} ID`);
  if (
    asset.name !== expectedName ||
    asset.state !== "uploaded" ||
    !Number.isSafeInteger(asset.size) ||
    asset.size <= 0 ||
    asset.size > MAX_ASSET_BYTES ||
    asset.url !== `${API_ORIGIN}/repos/${PUBLIC_RELEASE_REPOSITORY}/releases/assets/${asset.id}` ||
    asset.browser_download_url !==
      `${downloadBaseUrl}/${encodeURIComponent(expectedName)}`
  ) {
    throw new Error(`Draft asset ${expectedName} identity 无效`);
  }
  return asset;
}

/** 只允许 GitHub 官方 release-asset redirect；第二跳绝不携带 token。 */
function validateAssetRedirect(location) {
  let url;
  try { url = new URL(location); } catch { throw new Error("Draft asset 下载重定向无效"); }
  const allowedHost = ["release-assets.githubusercontent.com", "objects.githubusercontent.com"].includes(url.hostname);
  if (url.protocol !== "https:" || !allowedHost || url.username || url.password || url.port || url.hash) {
    throw new Error("Draft asset 下载重定向离开 GitHub 官方资产域名");
  }
  return url.href;
}

/** 取得一个 API asset 响应，手工剥离 Authorization 后跟随固定官方重定向。 */
async function requestAsset(asset, token, fetchImpl) {
  let response;
  try {
    response = await fetchImpl(asset.url, {
      redirect: "manual",
      headers: {
        Accept: "application/octet-stream",
        Authorization: `Bearer ${token}`,
        "X-GitHub-Api-Version": "2022-11-28",
      },
    });
  } catch {
    throw new Error(`Draft asset ${asset.name} API 下载失败`);
  }
  if ([301, 302, 303, 307, 308].includes(response.status)) {
    const redirected = validateAssetRedirect(response.headers.get("location"));
    try { await response.body?.cancel(); } catch { /* redirect 校验错误优先。 */ }
    try {
      response = await fetchImpl(redirected, {
        redirect: "error",
        headers: { Accept: "application/octet-stream" },
      });
    } catch {
      throw new Error(`Draft asset ${asset.name} 官方资产下载失败`);
    }
  }
  if (!response || response.redirected || response.status < 200 || response.status >= 300) {
    try { await response?.body?.cancel(); } catch { /* HTTP 错误优先。 */ }
    throw new Error(`Draft asset ${asset.name} 返回 HTTP ${response?.status ?? "unknown"}`);
  }
  return response;
}

/** 流式写入一个 asset，同时校验完整字节数并计算 SHA-256。 */
async function downloadAsset(asset, destination, token, fetchImpl) {
  const response = await requestAsset(asset, token, fetchImpl);
  const declared = response.headers.get("content-length");
  if (declared !== null && declared !== String(asset.size)) {
    try { await response.body?.cancel(); } catch { /* length 错误优先。 */ }
    throw new Error(`Draft asset ${asset.name} Content-Length 不匹配`);
  }
  if (!response.body) {
    throw new Error(`Draft asset ${asset.name} 缺少响应体`);
  }
  const hash = createHash("sha256");
  let bytes = 0;
  const verifier = new Transform({
    /** 逐块累计字节数与摘要，并在写盘前拒绝超过 API identity 的响应。 */
    transform(chunk, _encoding, callback) {
      bytes += chunk.length;
      if (bytes > asset.size) {
        callback(new Error(`Draft asset ${asset.name} 字节数超过 API identity`));
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
    if (bytes !== asset.size) throw new Error(`Draft asset ${asset.name} 字节数不匹配`);
  } catch (error) {
    await rm(destination, { force: true }).catch(() => {});
    throw error;
  }
  return { id: asset.id, name: asset.name, size: bytes, sha256: hash.digest("hex") };
}

/** 严格解析只覆盖其余七个资产的 SHA256SUMS。 */
function parseChecksums(text, expectedNames) {
  const lines = text.split("\n");
  if (lines.at(-1) !== "") throw new Error("SHA256SUMS 必须以换行结束");
  const entries = lines.slice(0, -1).map((line) => {
    const match = /^([0-9a-f]{64})[ ]{2}([^/\\\r\n]+)$/.exec(line);
    if (!match) throw new Error("SHA256SUMS 行格式无效");
    return { sha256: match[1], name: match[2] };
  });
  assertExactNameSet(entries.map(({ name }) => name), expectedNames, "SHA256SUMS 名称集合");
  return new Map(entries.map(({ name, sha256 }) => [name, sha256]));
}

/** 对八个本地 Draft bytes 做 manifest、checksum、签名和 URL 全合约复核。 */
async function verifyDownloadedDraft(directory, release, version, releaseSigningPublicKey, receipts) {
  const names = releaseAssetNames(version);
  const byName = new Map(receipts.map((asset) => [asset.name, asset]));
  const checksumText = await readFile(join(directory, names.checksums), "utf8");
  const seven = expectedDraftAssetNames(version).filter((name) => name !== names.checksums);
  const sums = parseChecksums(checksumText, seven);
  for (const name of seven) {
    if (byName.get(name)?.sha256 !== sums.get(name)) {
      throw new Error(`SHA256SUMS 与 ${name} 完整字节不匹配`);
    }
  }
  let manifest;
  try { manifest = JSON.parse(await readFile(join(directory, names.manifest), "utf8")); }
  catch { throw new Error("latest.json 不是有效 JSON"); }
  const signatures = new Map([
    [`${names.updater.darwin}.sig`, await readFile(join(directory, `${names.updater.darwin}.sig`), "utf8")],
    [`${names.updater.windows}.sig`, await readFile(join(directory, `${names.updater.windows}.sig`), "utf8")],
  ]);
  validateUpdaterManifest(manifest, {
    tag: release.tag_name,
    version,
    notes: release.body,
    signatures,
    assetDigests: sums,
  });
  await Promise.all([
    verifyTauriUpdaterSignature(
      await readFile(join(directory, names.updater.darwin)),
      signatures.get(`${names.updater.darwin}.sig`),
      releaseSigningPublicKey,
    ),
    verifyTauriUpdaterSignature(
      await readFile(join(directory, names.updater.windows)),
      signatures.get(`${names.updater.windows}.sig`),
      releaseSigningPublicKey,
    ),
  ]);
  return manifest;
}

/**
 * 从固定 public Release ID 回读完整 Draft；token 只进入 Authorization header，
 * receipt 只包含公开 identity、长度和完整 SHA-256。
 */
export async function downloadVerifiedPublicDraft(options, dependencies = {}) {
  const fetchImpl = dependencies.fetchImpl ?? globalThis.fetch;
  const {
    releaseId,
    tag,
    version,
    outputDirectory,
    token,
    mode,
    releaseSigningPublicKey,
    candidateUpdaterPublicKey,
    bridgeTag,
  } = options;
  if (typeof fetchImpl !== "function") throw new Error("当前 Node 缺少 fetch");
  if (typeof token !== "string" || !token || token.trim() !== token) {
    throw new Error("读取 public Draft 需要有效 CYUNLAB_RELEASE_TOKEN");
  }
  const updaterTrust = resolveUpdaterKeyPolicy({
    tag,
    mode,
    releaseSigningPublicKey,
    candidateUpdaterPublicKey,
    bridgeTag,
  });
  parseReleaseIdentity(tag, version);
  requireDatabaseId(releaseId, "public Release ID");
  await mkdir(outputDirectory, { recursive: true });
  const release = validateDraftReleaseIdentity(
    await githubApiJson(`/repos/${PUBLIC_RELEASE_REPOSITORY}/releases/${releaseId}`, token, fetchImpl),
    { releaseId, tag },
  );
  const downloadBaseUrl = draftDownloadBaseUrl(release);
  const expectedNames = expectedDraftAssetNames(version);
  assertExactNameSet((release.assets ?? []).map((asset) => asset.name), expectedNames, "public Draft 资产集合");
  const byName = new Map(release.assets.map((asset) => [asset.name, asset]));
  const receipts = [];
  try {
    for (const name of expectedNames) {
      const asset = validateAssetIdentity(byName.get(name), downloadBaseUrl, name);
      receipts.push(await downloadAsset(asset, join(outputDirectory, name), token, fetchImpl));
    }
    const manifest = await verifyDownloadedDraft(
      outputDirectory,
      release,
      version,
      releaseSigningPublicKey,
      receipts,
    );
    return Object.freeze({
      schemaVersion: 1,
      repository: PUBLIC_RELEASE_REPOSITORY,
      releaseId,
      tag,
      version,
      manifestSha256: receipts.find((asset) => asset.name === "latest.json").sha256,
      updaterTrust,
      assets: receipts.sort((left, right) => left.name.localeCompare(right.name, "en")),
      manifest,
    });
  } catch (error) {
    for (const name of expectedNames) await rm(join(outputDirectory, name), { force: true }).catch(() => {});
    throw error;
  }
}

/** 比较两份 Draft receipt；任何 asset ID/size/digest 漂移都拒绝。 */
export function assertSameDraftReceipt(actual, expected) {
  for (const field of ["repository", "releaseId", "tag", "version", "manifestSha256"]) {
    if (actual?.[field] !== expected?.[field]) throw new Error(`Draft receipt ${field} 已变化`);
  }
  const serialize = (value) => JSON.stringify(value?.assets ?? []);
  if (serialize(actual) !== serialize(expected)) throw new Error("Draft asset IDs/sizes/digests 已变化");
  if (JSON.stringify(actual?.updaterTrust) !== JSON.stringify(expected?.updaterTrust)) {
    throw new Error("Draft updater key identity 已变化");
  }
  return true;
}

/** CLI 仅供 workflow 下载固定 Draft；成功输出不包含 token 或下载 URL。 */
async function main() {
  const [releaseIdText, tag, version, outputDirectory, receiptPath] = process.argv.slice(2);
  const result = await downloadVerifiedPublicDraft({
    releaseId: Number(releaseIdText),
    tag,
    version,
    outputDirectory,
    token: process.env.CYUNLAB_RELEASE_TOKEN,
    mode: process.env.NARRANT_UPDATE_MODE,
    releaseSigningPublicKey: process.env.NARRANT_RELEASE_SIGNING_PUBLIC_KEY,
    candidateUpdaterPublicKey: process.env.NARRANT_UPDATER_PUBLIC_KEY,
    bridgeTag: process.env.NARRANT_UPDATER_BRIDGE_TAG,
  });
  await import("node:fs/promises").then(({ writeFile }) =>
    writeFile(receiptPath, `${JSON.stringify(result, null, 2)}\n`, { flag: "wx", mode: 0o600 }),
  );
  process.stdout.write(`已验证 public Draft ${tag}（8 个固定资产）。\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`错误：${error instanceof Error ? error.message : "public Draft 验证失败"}\n`);
    process.exitCode = 1;
  });
}

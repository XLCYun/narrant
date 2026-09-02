import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import process from "node:process";
import { pathToFileURL } from "node:url";

import {
  downloadVerifiedPublicDraft,
  expectedDraftAssetNames,
  FIXED_STABLE_ENDPOINT,
  PUBLIC_RELEASE_REPOSITORY,
  validateDraftReleaseIdentity,
} from "./public-draft-assets.mjs";
import { verifyUpdateSmokeEvidenceDirectory } from "./update-smoke-evidence.mjs";
import { resolvePreviousReleaseSigningKeySha256 } from "./release-assets.mjs";

const API_ORIGIN = "https://api.github.com";
const RESPONSE_BOUND = 512 * 1024;
const RELEASES_PER_PAGE = 100;
const MAX_RELEASE_PAGES = 20;

/** 解析一个有界 GitHub API JSON response，不把 body/token 注入错误。 */
async function parseApiResponse(response, context, allow404 = false) {
  if (allow404 && response.status === 404) {
    try { await response.body?.cancel(); } catch { /* 404 身份已经足够。 */ }
    return undefined;
  }
  if (!response || response.redirected || response.status < 200 || response.status >= 300) {
    try { await response?.body?.cancel(); } catch { /* HTTP status 主错误优先。 */ }
    throw new Error(`${context}返回 HTTP ${response?.status ?? "unknown"}`);
  }
  const type = response.headers.get("content-type") ?? "";
  const length = response.headers.get("content-length");
  if (!/^application\/json(?:\s*;|$)/i.test(type) || (length !== null && (!/^\d+$/.test(length) || Number(length) > RESPONSE_BOUND))) {
    try { await response.body?.cancel(); } catch { /* response metadata 主错误优先。 */ }
    throw new Error(`${context}响应 metadata 无效`);
  }
  const bytes = Buffer.from(await response.arrayBuffer());
  if (bytes.length > RESPONSE_BOUND) throw new Error(`${context}响应超过上限`);
  try { return JSON.parse(bytes.toString("utf8")); }
  catch { throw new Error(`${context}返回无效 JSON`); }
}

/** token 只进入 Authorization header 的 GitHub API adapter。 */
async function githubApi(pathname, token, fetchImpl, options = {}) {
  let response;
  try {
    response = await fetchImpl(`${API_ORIGIN}${pathname}`, {
      method: options.method ?? "GET",
      redirect: "error",
      headers: {
        Accept: "application/vnd.github+json",
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
        "X-GitHub-Api-Version": "2022-11-28",
      },
      ...(options.body === undefined ? {} : { body: JSON.stringify(options.body) }),
    });
  } catch {
    throw new Error(`${options.context ?? "GitHub API"}网络请求失败`);
  }
  return parseApiResponse(response, options.context ?? "GitHub API", options.allow404 === true);
}

/** 匿名回读当前 shipped Stable manifest，用于 normal previous identity 二次校验。 */
async function fetchStableManifest(fetchImpl) {
  let response;
  try { response = await fetchImpl(FIXED_STABLE_ENDPOINT, { redirect: "follow" }); }
  catch { throw new Error("当前 Stable manifest 回读失败"); }
  if (!response || response.status < 200 || response.status >= 300) throw new Error("当前 Stable manifest 不可用");
  const bytes = Buffer.from(await response.arrayBuffer());
  if (bytes.length <= 0 || bytes.length > RESPONSE_BOUND) throw new Error("当前 Stable manifest 大小无效");
  let manifest;
  try { manifest = JSON.parse(bytes.toString("utf8")); } catch { throw new Error("当前 Stable manifest 不是 JSON"); }
  return { manifest, sha256: createHash("sha256").update(bytes).digest("hex") };
}

/** 比较 mutation 前 API asset identity 与已完成 byte 验证的 receipt。 */
function assertDraftAssetIdentity(release, receipt) {
  const expectedNames = expectedDraftAssetNames(receipt.version).sort();
  const assets = release.assets ?? [];
  const actualNames = assets.map((asset) => asset.name).sort();
  if (actualNames.length !== expectedNames.length || actualNames.some((name, index) => name !== expectedNames[index])) {
    throw new Error("promotion 前 Draft asset 名称集合已变化");
  }
  const expectedByName = new Map(receipt.assets.map((asset) => [asset.name, asset]));
  for (const asset of assets) {
    const expected = expectedByName.get(asset.name);
    if (asset.id !== expected.id || asset.size !== expected.size || asset.state !== "uploaded") {
      throw new Error("promotion 前 Draft asset ID/size/state 已变化");
    }
  }
}

/** bootstrap 只允许 protected variable 指定的首版。 */
export function assertBootstrapTransition(candidateTag, bootstrapTag) {
  if (!bootstrapTag || candidateTag !== bootstrapTag) throw new Error("bootstrap tag 不匹配 protected Environment variable");
  return true;
}

/** 校验一个 Release 历史页，并永久拒绝任一已发布 updater manifest。 */
function inspectBootstrapHistoryPage(releases, options, seenIds) {
  if (!Array.isArray(releases) || releases.length > RELEASES_PER_PAGE) throw new Error("bootstrap Release 历史页无效");
  for (const release of releases) {
    if (!Number.isSafeInteger(release?.id) || release.id <= 0 || seenIds.has(release.id)) {
      throw new Error("bootstrap Release 历史 identity 无效或重复");
    }
    seenIds.add(release.id);
    if (typeof release.tag_name !== "string" || typeof release.draft !== "boolean" || !Array.isArray(release.assets)) {
      throw new Error("bootstrap Release 历史 shape 无效");
    }
    if (release.id === options.candidateReleaseId) {
      if (release.tag_name !== options.candidateTag || release.draft !== true) {
        throw new Error("bootstrap candidate 在 Release 历史中 identity 漂移");
      }
      continue;
    }
    if (release.draft === false && release.assets.some((asset) => asset?.name === "latest.json")) {
      throw new Error("已有历史 updater-capable published Stable，bootstrap 已永久关闭");
    }
  }
}

/** 有界分页扫描固定 public repository 的完整 Release 历史。 */
export async function verifyBootstrapReleaseHistory(options, dependencies = {}) {
  const fetchImpl = dependencies.fetchImpl ?? globalThis.fetch;
  const seenIds = new Set();
  for (let page = 1; page <= MAX_RELEASE_PAGES; page += 1) {
    const releases = await githubApi(
      `/repos/${PUBLIC_RELEASE_REPOSITORY}/releases?per_page=${RELEASES_PER_PAGE}&page=${page}`,
      options.token,
      fetchImpl,
      { context: `扫描 bootstrap Release 历史第 ${page} 页` },
    );
    inspectBootstrapHistoryPage(releases, options, seenIds);
    if (releases.length < RELEASES_PER_PAGE) return true;
  }
  throw new Error("bootstrap Release 历史超过有界分页上限");
}

/**
 * 二次下载/验签固定 Draft 与 evidence 后发布 exact Release ID；任何验证失败都发生在
 * PATCH mutation 之前，已发布 Release 永远不进入此路径。
 */
export async function promoteStableRelease(options, dependencies = {}) {
  const fetchImpl = dependencies.fetchImpl ?? globalThis.fetch;
  const downloadDraft = dependencies.downloadDraft ?? downloadVerifiedPublicDraft;
  const verifyEvidence = dependencies.verifyEvidence ?? verifyUpdateSmokeEvidenceDirectory;
  const token = options.token;
  if (typeof token !== "string" || !token || token.trim() !== token) throw new Error("promotion 缺少 CYUNLAB_RELEASE_TOKEN");
  if (!["normal", "bootstrap"].includes(options.mode)) throw new Error("promotion mode 无效");
  const previousReleaseSigningKeySha256 = resolvePreviousReleaseSigningKeySha256(
    options.mode,
    options.previousReleaseSigningPublicKey,
  );
  const draftReceipt = await downloadDraft({
    releaseId: options.publicReleaseId,
    tag: options.publicTag,
    version: options.version,
    outputDirectory: options.draftDirectory,
    token,
    mode: options.mode,
    releaseSigningPublicKey: options.releaseSigningPublicKey,
    candidateUpdaterPublicKey: options.candidateUpdaterPublicKey,
    bridgeTag: options.bridgeTag,
  });
  let stableManifest;
  if (options.mode === "bootstrap") {
    assertBootstrapTransition(options.sourceTag, options.bootstrapTag);
    await (dependencies.verifyBootstrapHistory ?? verifyBootstrapReleaseHistory)({
      token,
      candidateReleaseId: options.publicReleaseId,
      candidateTag: options.publicTag,
    }, { fetchImpl });
  } else {
    const latestBefore = await githubApi(`/repos/${PUBLIC_RELEASE_REPOSITORY}/releases/latest`, token, fetchImpl, {
      allow404: true, context: "查询 promotion 前 Latest",
    });
    if (!latestBefore || latestBefore.tag_name !== options.previousTag || latestBefore.draft === true) {
      throw new Error("promotion 前 Latest 不匹配 previous Stable");
    }
    stableManifest = await (dependencies.fetchStableManifest ?? fetchStableManifest)(fetchImpl);
    if (stableManifest.manifest.version !== options.previousTag.replace(/^v/, "") || stableManifest.sha256 !== options.previousManifestSha256) {
      throw new Error("promotion 前 Stable manifest identity 已变化");
    }
  }
  await verifyEvidence(options.evidenceDirectory, {
    mode: options.mode,
    sourceTag: options.sourceTag,
    sourceSha: options.sourceSha,
    publicReleaseId: options.publicReleaseId,
    publicTag: options.publicTag,
    version: options.version,
    manifestSha256: draftReceipt.manifestSha256,
    draftReceipt,
    previousTag: options.previousTag,
    previousManifestSha256: options.previousManifestSha256,
    previousReleaseSigningKeySha256,
    allowBootstrap: options.mode === "bootstrap",
    bootstrapTag: options.bootstrapTag,
    maxAgeHours: options.maxAgeHours,
    now: options.now,
  });

  const immediatelyBefore = validateDraftReleaseIdentity(
    await githubApi(`/repos/${PUBLIC_RELEASE_REPOSITORY}/releases/${options.publicReleaseId}`, token, fetchImpl, {
      context: "promotion 前固定 ID Draft 回读",
    }),
    { releaseId: options.publicReleaseId, tag: options.publicTag },
  );
  assertDraftAssetIdentity(immediatelyBefore, draftReceipt);
  const published = await githubApi(`/repos/${PUBLIC_RELEASE_REPOSITORY}/releases/${options.publicReleaseId}`, token, fetchImpl, {
    method: "PATCH",
    body: { draft: false, prerelease: false, make_latest: "true" },
    context: "发布固定 ID Draft",
  });
  if (
    published?.id !== options.publicReleaseId || published.tag_name !== options.publicTag ||
    published.draft !== false || published.prerelease !== false
  ) {
    throw new Error("GitHub 未确认 fixed-ID Stable publication");
  }
  const latestAfter = await githubApi(`/repos/${PUBLIC_RELEASE_REPOSITORY}/releases/latest`, token, fetchImpl, {
    context: "publication 后 Latest 回读",
  });
  if (latestAfter?.id !== options.publicReleaseId || latestAfter.tag_name !== options.publicTag || latestAfter.draft !== false) {
    throw new Error("published Release 未成为 Latest");
  }
  return Object.freeze({ releaseId: published.id, tag: published.tag_name, mode: options.mode });
}

/** 解析 workflow CLI 的成对参数。 */
function parseArguments(arguments_) {
  const values = {};
  for (let index = 0; index < arguments_.length; index += 2) {
    const name = arguments_[index];
    const value = arguments_[index + 1];
    if (!name?.startsWith("--") || value === undefined) throw new Error(`无效 promotion 参数：${name ?? ""}`);
    values[name.slice(2).replace(/-([a-z])/g, (_match, letter) => letter.toUpperCase())] = value;
  }
  return values;
}

/** 从受保护 workflow 环境读取 promotion 输入并发布固定 public Release。 */
async function main() {
  const values = parseArguments(process.argv.slice(2));
  const result = await promoteStableRelease({
    ...values,
    publicReleaseId: Number(values.publicReleaseId),
    maxAgeHours: Number(values.maxAgeHours),
    token: process.env.CYUNLAB_RELEASE_TOKEN,
    releaseSigningPublicKey: process.env.NARRANT_RELEASE_SIGNING_PUBLIC_KEY,
    candidateUpdaterPublicKey: process.env.NARRANT_UPDATER_PUBLIC_KEY,
    bridgeTag: process.env.NARRANT_UPDATER_BRIDGE_TAG,
    previousReleaseSigningPublicKey: process.env.NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY,
  });
  process.stdout.write(`已发布 public Stable ${result.tag}（Release ${result.releaseId}）。\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`错误：${error instanceof Error ? error.message : "promotion 失败"}\n`);
    process.exitCode = 1;
  });
}

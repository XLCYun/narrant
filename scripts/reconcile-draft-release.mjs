#!/usr/bin/env node

import { spawn } from "node:child_process";
import { Buffer } from "node:buffer";
import { openAsBlob as openBlobFromDisk } from "node:fs";
import { basename } from "node:path";
import { pathToFileURL } from "node:url";
import process from "node:process";
import { TextDecoder } from "node:util";

import {
  assertDraftArtifactPaths,
  assertExactNameSet,
  parseReleaseIdentity,
} from "./release-assets.mjs";

const PUBLIC_REPOSITORY = "cyunlab/narrant";
const MAX_UPLOAD_RESPONSE_BYTES = 256 * 1024;
const RELEASE_LOOKUP_PAGE_SIZE = 100;
const MAX_RELEASE_LOOKUP_PAGES = 100;

/** 要求 gh 命令成功，并把安全的标准错误传给调用方。 */
async function requireSuccess(resultPromise, context) {
  const result = await resultPromise;
  if (result.exitCode !== 0) {
    throw new Error(`${context}失败：${result.stderr.trim() || `gh 退出码 ${result.exitCode}`}`);
  }
  return result;
}

/** 解析 GitHub Release API 输出，拒绝非对象或缺少关键字段的响应。 */
function parseRelease(result, context) {
  let release;
  try {
    release = JSON.parse(result.stdout);
  } catch {
    throw new Error(`${context}返回无效 JSON`);
  }
  if (
    !release ||
    typeof release.id !== "number" ||
    typeof release.draft !== "boolean" ||
    typeof release.tag_name !== "string"
  ) {
    throw new Error(`${context}缺少 Release 身份字段`);
  }
  return release;
}

/** 要求回读对象仍是同一个 tag、同一个 ID 且保持 Draft。 */
function assertDraftIdentity(release, tag, expectedId, context) {
  if (release.id !== expectedId || release.tag_name !== tag) {
    throw new Error(`${context}的 Release ID/tag 已变化`);
  }
  if (!release.draft) {
    throw new Error(`${context}拒绝修改已发布 Release ${tag}`);
  }
}

/** 从 GitHub Draft 身份推导该 Release 当下真实的 untagged 下载根路径。 */
function draftDownloadBaseUrl(release, repository, releaseId) {
  const expectedApiUrl = `https://api.github.com/repos/${repository}/releases/${releaseId}`;
  const expectedUploadUrl = `https://uploads.github.com/repos/${repository}/releases/${releaseId}/assets{?name,label}`;
  const htmlPrefix = `https://github.com/${repository}/releases/tag/`;
  if (
    release.url !== expectedApiUrl ||
    release.upload_url !== expectedUploadUrl ||
    typeof release.html_url !== "string" ||
    !release.html_url.startsWith(htmlPrefix)
  ) {
    throw new Error("Draft Release 返回错误仓库或上传身份");
  }
  const draftIdentity = release.html_url.slice(htmlPrefix.length);
  if (!/^untagged-[A-Za-z0-9]+$/.test(draftIdentity)) {
    throw new Error("Draft Release 返回无效 untagged 身份");
  }
  return `https://github.com/${repository}/releases/download/${draftIdentity}`;
}

/** 解析 Release 列表响应，拒绝非数组结构。 */
function parseReleaseList(result, context) {
  let releases;
  try {
    releases = JSON.parse(result.stdout);
  } catch {
    throw new Error(`${context}返回无效 JSON`);
  }
  if (!Array.isArray(releases)) {
    throw new Error(`${context}返回非数组结构`);
  }
  return releases;
}

/**
 * 读取指定 tag 的 Release。GitHub 的 tag endpoint 不返回 Draft，
 * 因此在明确 404 后必须分页扫描已授权列表，避免重复创建同 tag Draft。
 */
async function lookupRelease(runGh, repository, tag) {
  const result = await runGh([
    "api",
    `repos/${repository}/releases/tags/${encodeURIComponent(tag)}`,
  ]);
  if (result.exitCode === 0) {
    return parseRelease(result, "Release 查询");
  }
  if (!/HTTP 404\b/i.test(result.stderr)) {
    throw new Error(`无法检查 Release ${tag}：${result.stderr.trim() || `gh 退出码 ${result.exitCode}`}`);
  }

  let matched;
  for (let page = 1; page <= MAX_RELEASE_LOOKUP_PAGES; page += 1) {
    const pageResult = await requireSuccess(
      runGh([
        "api",
        `repos/${repository}/releases?per_page=${RELEASE_LOOKUP_PAGE_SIZE}&page=${page}`,
      ]),
      `分页查询 Draft Release（第 ${page} 页）`,
    );
    const releases = parseReleaseList(pageResult, `Release 列表第 ${page} 页`);
    for (const candidate of releases) {
      if (candidate?.tag_name !== tag) continue;
      const parsed = parseRelease(
        { stdout: JSON.stringify(candidate) },
        `Release 列表第 ${page} 页`,
      );
      if (matched && matched.id !== parsed.id) {
        throw new Error(`Release 列表中存在重复 tag ${tag}`);
      }
      matched = parsed;
    }
    if (releases.length < RELEASE_LOOKUP_PAGE_SIZE) return matched;
  }
  throw new Error(`Release 列表超过 ${MAX_RELEASE_LOOKUP_PAGES} 页，拒绝不完整查询 ${tag}`);
}

/** 以 immutable release ID 紧邻 mutation 回读并复核 Draft 身份。 */
async function readDraftById(runGh, repository, tag, releaseId, context) {
  const result = await requireSuccess(
    runGh(["api", `repos/${repository}/releases/${releaseId}`]),
    `${context}回读 Draft`,
  );
  const release = parseRelease(result, `${context}回读 Draft`);
  assertDraftIdentity(release, tag, releaseId, context);
  return release;
}

/** 要求 API 回读的 Release 仍是目标 Draft 且资产名集合完全一致。 */
function assertUploadedDraft(release, tag, releaseId, expectedNames) {
  assertDraftIdentity(release, tag, releaseId, "上传后");
  const actualNames = (release.assets ?? []).map((asset) => asset.name);
  assertExactNameSet(actualNames, expectedNames, "上传后 Draft 资产集合");
}

/** 验证 uploads.github.com 对单个二进制资产返回的固定仓库、名称、字节数和下载身份。 */
function parseUploadedAsset(asset, { repository, downloadBaseUrl, name, expectedSize }) {
  if (
    !asset ||
    !Number.isSafeInteger(asset.id) ||
    asset.id <= 0 ||
    asset.name !== name ||
    asset.state !== "uploaded" ||
    asset.content_type !== "application/octet-stream" ||
    !Number.isSafeInteger(asset.size) ||
    asset.size !== expectedSize
  ) {
    throw new Error(`上传资产 ${name} 返回无效 asset identity`);
  }
  const expectedApiUrl = `https://api.github.com/repos/${repository}/releases/assets/${asset.id}`;
  const expectedDownloadUrl = `${downloadBaseUrl}/${encodeURIComponent(name)}`;
  if (
    asset.url !== expectedApiUrl ||
    asset.browser_download_url !== expectedDownloadUrl
  ) {
    throw new Error(`上传资产 ${name} 返回错误仓库或下载 URL`);
  }
  return asset;
}

/** 尽力取消尚未交给 reader 的响应体；清理异常永远不暴露也不覆盖主错误。 */
async function cancelResponseBody(body) {
  try {
    if (body && typeof body.cancel === "function") await body.cancel();
  } catch {
    // 远端 lifecycle cause 可能包含敏感 header；调用方只保留原本的安全错误。
  }
}

/** 尽力取消 reader；清理异常永远不暴露也不覆盖主错误。 */
async function cancelResponseReader(reader) {
  try {
    await reader.cancel();
  } catch {
    // reader 的底层 cause 不进入 Actions 错误输出。
  }
}

/** 尽力释放 reader lock，并用布尔值让成功路径感知 lifecycle 失败。 */
function releaseResponseReader(reader) {
  try {
    reader.releaseLock();
    return true;
  } catch {
    return false;
  }
}

/** 取消未消费 body 后抛出预先构造的安全主错误。 */
async function rejectUploadResponse(body, message) {
  await cancelResponseBody(body);
  throw new Error(message);
}

/** 验证响应 metadata，并在固定上限内读取；所有 lifecycle cause 都替换为安全中文错误。 */
async function readBoundedUploadResponse(response, name) {
  let body;
  let redirected;
  let status;
  let contentType;
  let contentLength;
  try {
    body = response.body;
    redirected = response.redirected;
    status = response.status;
    contentType = response.headers.get("content-type");
    contentLength = response.headers.get("content-length");
  } catch {
    await cancelResponseBody(body);
    throw new Error(`上传资产 ${name} 的响应 metadata 无法安全读取`);
  }
  if (typeof redirected !== "boolean" || !Number.isSafeInteger(status)) {
    await rejectUploadResponse(body, `上传资产 ${name} 的响应 metadata 无效`);
  }
  if (redirected) {
    await rejectUploadResponse(body, `上传 Draft 资产 ${name} 拒绝重定向响应`);
  }
  if (status < 200 || status >= 300) {
    await rejectUploadResponse(body, `上传 Draft 资产 ${name} 失败（HTTP ${status}）`);
  }
  if (typeof contentType !== "string" || !/^application\/json(?:\s*;|$)/i.test(contentType)) {
    await rejectUploadResponse(body, `上传资产 ${name} 返回错误 Content-Type`);
  }
  if (contentLength !== null) {
    if (typeof contentLength !== "string") {
      await rejectUploadResponse(body, `上传资产 ${name} 返回无效 Content-Length`);
    }
    const parsedLength = Number(contentLength);
    if (!Number.isSafeInteger(parsedLength) || parsedLength < 0) {
      await rejectUploadResponse(body, `上传资产 ${name} 返回无效 Content-Length`);
    }
    if (parsedLength > MAX_UPLOAD_RESPONSE_BYTES) {
      await rejectUploadResponse(body, `上传资产 ${name} 的响应超过安全上限`);
    }
  }
  let getReader;
  try {
    getReader = body?.getReader;
  } catch {
    await cancelResponseBody(body);
    throw new Error(`上传资产 ${name} 的响应 reader 无法安全创建`);
  }
  if (typeof getReader !== "function") {
    await rejectUploadResponse(body, `上传资产 ${name} 缺少可读取响应体`);
  }

  let reader;
  try {
    reader = getReader.call(body);
  } catch {
    await cancelResponseBody(body);
    throw new Error(`上传资产 ${name} 的响应 reader 无法安全创建`);
  }
  const chunks = [];
  let totalBytes = 0;
  let primaryError;
  let completed = false;
  while (!primaryError && !completed) {
    let result;
    try {
      result = await reader.read();
    } catch {
      primaryError = new Error(`上传资产 ${name} 的响应读取失败`);
      break;
    }
    try {
      if (result.done) {
        completed = true;
        break;
      }
      const chunk = Buffer.from(result.value);
      totalBytes += chunk.byteLength;
      if (totalBytes > MAX_UPLOAD_RESPONSE_BYTES) {
        primaryError = new Error(`上传资产 ${name} 的响应超过安全上限`);
        break;
      }
      chunks.push(chunk);
    } catch {
      primaryError = new Error(`上传资产 ${name} 的响应 reader lifecycle 无效`);
    }
  }
  if (primaryError || !completed) await cancelResponseReader(reader);
  if (!releaseResponseReader(reader) && !primaryError) {
    await cancelResponseReader(reader);
    primaryError = new Error(`上传资产 ${name} 的响应 reader lifecycle 无效`);
  }
  if (primaryError) throw primaryError;

  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(Buffer.concat(chunks));
  } catch {
    throw new Error(`上传资产 ${name} 返回无效 UTF-8`);
  }
}

/** 直连官方 release-ID endpoint 上传一个资产；令牌只进入 Authorization header。 */
export async function uploadReleaseAsset(options, dependencies = {}) {
  const { repository, tag, releaseId, artifact, token } = options;
  const downloadBaseUrl = options.downloadBaseUrl ??
    `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}`;
  const fetchImpl = dependencies.fetchImpl ?? globalThis.fetch;
  const openBlob = dependencies.openBlob ?? openBlobFromDisk;
  if (repository !== PUBLIC_REPOSITORY || !Number.isSafeInteger(releaseId) || releaseId <= 0) {
    throw new Error("上传资产的仓库或 Release ID 无效");
  }
  const allowedDownloadPrefix = `https://github.com/${repository}/releases/download/`;
  const downloadIdentity = typeof downloadBaseUrl === "string" &&
    downloadBaseUrl.startsWith(allowedDownloadPrefix)
    ? downloadBaseUrl.slice(allowedDownloadPrefix.length)
    : "";
  if (
    !downloadIdentity ||
    downloadIdentity.includes("/") ||
    encodeURIComponent(downloadIdentity) !== downloadIdentity
  ) {
    throw new Error("上传资产的 Draft 下载身份无效");
  }
  if (typeof token !== "string" || token.length === 0 || token.trim() !== token) {
    throw new Error("上传 Draft 资产需要有效 GH_TOKEN");
  }
  if (typeof fetchImpl !== "function") {
    throw new Error("当前 Node 运行时不支持安全上传");
  }

  const name = basename(artifact);
  const endpoint = `https://uploads.github.com/repos/${repository}/releases/${releaseId}/assets?name=${encodeURIComponent(name)}`;
  let body;
  let expectedSize;
  try {
    body = await openBlob(artifact, { type: "application/octet-stream" });
    expectedSize = body.size;
    if (!Number.isSafeInteger(expectedSize) || expectedSize <= 0) throw new Error("invalid blob size");
  } catch {
    throw new Error(`无法读取待上传资产 ${name}`);
  }

  let response;
  try {
    response = await fetchImpl(endpoint, {
      method: "POST",
      redirect: "error",
      headers: {
        Accept: "application/vnd.github+json",
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/octet-stream",
        "X-GitHub-Api-Version": "2022-11-28",
      },
      body,
    });
  } catch {
    throw new Error(`上传 Draft 资产 ${name} 的网络请求失败`);
  }
  const responseText = await readBoundedUploadResponse(response, name);
  let asset;
  try {
    asset = JSON.parse(responseText);
  } catch {
    throw new Error(`上传资产 ${name} 返回无效 JSON`);
  }
  return parseUploadedAsset(asset, { repository, downloadBaseUrl, name, expectedSize });
}

/**
 * 创建或完整替换一个公开 Draft；所有远端状态变化都隐藏在可注入的 gh adapter 后。
 * 已发布 Release、非 404 查询错误和上传后集合偏差都会 fail closed。
 */
export async function reconcileDraftRelease(options, dependencies = {}) {
  const runGh = dependencies.runGh ?? runGhCommand;
  const uploadAsset = dependencies.uploadAsset ?? uploadReleaseAsset;
  const token = dependencies.token ?? process.env.GH_TOKEN;
  const { repository, tag, version, title, notesFile, artifacts } = options;
  if (repository !== PUBLIC_REPOSITORY) {
    throw new Error(`只允许协调公开仓库 ${PUBLIC_REPOSITORY}`);
  }
  parseReleaseIdentity(tag, version);
  if (title !== `Narrant ${version}` || !notesFile) {
    throw new Error("Draft 标题或 Release Notes 文件无效");
  }
  assertDraftArtifactPaths(artifacts, version);

  const current = await lookupRelease(runGh, repository, tag);
  let outcome;
  let releaseId;
  if (current) {
    assertDraftIdentity(current, tag, current.id, "初始查询");
    releaseId = current.id;
    for (const asset of current.assets ?? []) {
      if (typeof asset.id !== "number") {
        throw new Error("Draft 资产缺少可验证的 asset id");
      }
      await readDraftById(runGh, repository, tag, releaseId, "删除旧资产前");
      await requireSuccess(
        runGh(["api", "--method", "DELETE", `repos/${repository}/releases/assets/${asset.id}`]),
        `删除 Draft 旧资产 ${asset.name ?? asset.id}`,
      );
      await readDraftById(runGh, repository, tag, releaseId, "删除旧资产后");
    }
    await readDraftById(runGh, repository, tag, releaseId, "更新元数据前");
    await requireSuccess(
      runGh([
        "api",
        "--method",
        "PATCH",
        `repos/${repository}/releases/${releaseId}`,
        "-f",
        `tag_name=${tag}`,
        "-f",
        `name=${title}`,
        "-F",
        `body=@${notesFile}`,
      ]),
      "更新 Draft 元数据",
    );
    await readDraftById(runGh, repository, tag, releaseId, "更新元数据后");
    outcome = "updated";
  } else {
    const createResult = await requireSuccess(
      runGh([
        "api",
        "--method",
        "POST",
        `repos/${repository}/releases`,
        "-f",
        `tag_name=${tag}`,
        "-f",
        `name=${title}`,
        "-F",
        `body=@${notesFile}`,
        "-F",
        "draft=true",
        "-F",
        "prerelease=false",
        "-F",
        "generate_release_notes=false",
      ]),
      "创建 Draft",
    );
    const created = parseRelease(createResult, "创建 Draft");
    assertDraftIdentity(created, tag, created.id, "创建后");
    releaseId = created.id;
    await readDraftById(runGh, repository, tag, releaseId, "创建后身份确认");
    outcome = "created";
  }

  for (const artifact of artifacts) {
    const name = basename(artifact);
    const guardedDraft = await readDraftById(runGh, repository, tag, releaseId, `上传 ${name} 前`);
    const downloadBaseUrl = draftDownloadBaseUrl(guardedDraft, repository, releaseId);
    await uploadAsset({ repository, tag, releaseId, artifact, token, downloadBaseUrl });
    await readDraftById(runGh, repository, tag, releaseId, `上传 ${name} 后`);
  }
  const uploaded = await readDraftById(runGh, repository, tag, releaseId, "全部上传后");
  assertUploadedDraft(
    uploaded,
    tag,
    releaseId,
    artifacts.map((artifact) => basename(artifact)),
  );
  return { outcome, releaseId: uploaded.id };
}

/** 不经 shell 启动 gh，避免 tag、路径或标题被二次解释。 */
export function runGhCommand(arguments_, dependencies = {}) {
  const spawnProcess = dependencies.spawnProcess ?? spawn;
  return new Promise((resolve) => {
    let child;
    try {
      child = spawnProcess("gh", arguments_, {
        stdio: ["ignore", "pipe", "pipe"],
        windowsHide: true,
      });
    } catch (error) {
      resolve({ exitCode: -1, stdout: "", stderr: error instanceof Error ? error.message : String(error) });
      return;
    }
    let stdout = "";
    let stderr = "";
    let processError;
    let settled = false;
    const settleOnClose = (code) => {
      if (settled) return;
      settled = true;
      resolve({
        exitCode: processError ? -1 : code ?? -1,
        stdout,
        stderr: processError ? [stderr, processError.message].filter(Boolean).join("\n") : stderr,
      });
    };
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.once("error", (error) => { processError = error; });
    child.once("close", settleOnClose);
  });
}

/** 解析 Actions 输入并协调一个经过本地预验的完整 Draft。 */
async function main() {
  const [tag, version, notesFile, ...artifacts] = process.argv.slice(2);
  const result = await reconcileDraftRelease({
    repository: process.env.TARGET_REPOSITORY,
    tag,
    version,
    title: `Narrant ${version}`,
    notesFile,
    artifacts,
  });
  process.stdout.write(`Draft ${tag} ${result.outcome}（Release ${result.releaseId}）。\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    await main();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`错误：${message}\n`);
    process.exitCode = 1;
  }
}

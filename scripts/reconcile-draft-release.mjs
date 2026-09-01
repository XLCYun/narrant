#!/usr/bin/env node

import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";
import process from "node:process";

import {
  assertDraftArtifactPaths,
  parseReleaseIdentity,
} from "./release-assets.mjs";

const PUBLIC_REPOSITORY = "cyunlab/narrant";

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
  if (!release || typeof release.id !== "number" || typeof release.draft !== "boolean") {
    throw new Error(`${context}缺少 Release 身份字段`);
  }
  return release;
}

/** 读取指定 tag 的 Release；只有明确的 HTTP 404 才表示尚未创建。 */
async function lookupRelease(runGh, repository, tag) {
  const result = await runGh([
    "api",
    `repos/${repository}/releases/tags/${encodeURIComponent(tag)}`,
  ]);
  if (result.exitCode === 0) {
    return parseRelease(result, "Release 查询");
  }
  if (/HTTP 404\b/i.test(result.stderr)) {
    return undefined;
  }
  throw new Error(`无法检查 Release ${tag}：${result.stderr.trim() || `gh 退出码 ${result.exitCode}`}`);
}

/** 要求 API 回读的 Release 仍是目标 Draft 且资产名集合完全一致。 */
function assertUploadedDraft(release, tag, expectedNames) {
  if (!release.draft) {
    throw new Error(`拒绝接受已发布 Release ${tag}`);
  }
  if (release.tag_name !== tag) {
    throw new Error(`Release tag 回读不匹配：${release.tag_name ?? "<missing>"}`);
  }
  const actualNames = (release.assets ?? []).map((asset) => asset.name);
  const counts = new Map();
  for (const name of actualNames) {
    counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  const duplicates = [...counts].filter(([, count]) => count > 1).map(([name]) => name);
  const expected = new Set(expectedNames);
  const missing = expectedNames.filter((name) => !counts.has(name));
  const unexpected = [...counts.keys()].filter((name) => !expected.has(name));
  if (duplicates.length || missing.length || unexpected.length) {
    throw new Error([
      "上传后 Draft 资产集合不精确",
      duplicates.length ? `重复：${duplicates.sort().join(", ")}` : "",
      missing.length ? `缺少：${missing.sort().join(", ")}` : "",
      unexpected.length ? `意外：${unexpected.sort().join(", ")}` : "",
    ].filter(Boolean).join("；"));
  }
}

/**
 * 创建或完整替换一个公开 Draft；所有远端状态变化都隐藏在可注入的 gh adapter 后。
 * 已发布 Release、非 404 查询错误和上传后集合偏差都会 fail closed。
 */
export async function reconcileDraftRelease(options, dependencies = {}) {
  const runGh = dependencies.runGh ?? runGhCommand;
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
  if (current) {
    if (!current.draft) {
      throw new Error(`拒绝修改已发布 Release ${tag}`);
    }
    for (const asset of current.assets ?? []) {
      if (typeof asset.id !== "number") {
        throw new Error("Draft 资产缺少可验证的 asset id");
      }
      await requireSuccess(
        runGh(["api", "--method", "DELETE", `repos/${repository}/releases/assets/${asset.id}`]),
        `删除 Draft 旧资产 ${asset.name ?? asset.id}`,
      );
    }
    await requireSuccess(
      runGh([
        "release",
        "edit",
        tag,
        "--repo",
        repository,
        "--draft",
        "--title",
        title,
        "--notes-file",
        notesFile,
      ]),
      "更新 Draft 元数据",
    );
    outcome = "updated";
  } else {
    await requireSuccess(
      runGh([
        "release",
        "create",
        tag,
        "--repo",
        repository,
        "--draft",
        "--title",
        title,
        "--notes-file",
        notesFile,
      ]),
      "创建 Draft",
    );
    outcome = "created";
  }

  await requireSuccess(
    runGh(["release", "upload", tag, ...artifacts, "--repo", repository, "--clobber"]),
    "上传 Draft 资产",
  );
  const uploaded = await lookupRelease(runGh, repository, tag);
  if (!uploaded) {
    throw new Error("上传后无法回读 Draft Release");
  }
  assertUploadedDraft(uploaded, tag, artifacts.map((artifact) => artifact.split(/[\\/]/).at(-1)));
  return { outcome, releaseId: uploaded.id };
}

/** 不经 shell 启动 gh，避免 tag、路径或标题被二次解释。 */
export function runGhCommand(arguments_) {
  return new Promise((resolve) => {
    const child = spawn("gh", arguments_, {
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.once("error", (error) => resolve({ exitCode: -1, stdout, stderr: error.message }));
    child.once("exit", (code) => resolve({ exitCode: code ?? -1, stdout, stderr }));
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

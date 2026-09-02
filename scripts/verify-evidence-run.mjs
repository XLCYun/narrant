import { Buffer } from "node:buffer";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { TextDecoder } from "node:util";

const API_ORIGIN = "https://api.github.com";
const PRIVATE_REPOSITORY = "XLCYun/narrant";
const WORKFLOW_NAME = "Native application update smoke evidence";
const WORKFLOW_PATH = ".github/workflows/update-smoke.yml";
const RELEASE_WORKFLOW_NAME = "release";
const RELEASE_WORKFLOW_PATH = ".github/workflows/release.yml";
const MAX_API_BYTES = 1024 * 1024;

/** 要求 GitHub database ID 是安全正整数。 */
function requireDatabaseId(value, label) {
  if (!Number.isSafeInteger(value) || value <= 0) throw new Error(`${label} 必须是正整数`);
  return value;
}

/** 有界读取 GitHub JSON，拒绝错误页或超大响应。 */
async function readBoundedJson(response, label) {
  const length = response.headers.get("content-length");
  if (length !== null && (!/^\d+$/.test(length) || Number(length) > MAX_API_BYTES)) {
    throw new Error(`${label} 响应超过安全上限`);
  }
  if (!response.body || typeof response.body.getReader !== "function") {
    throw new Error(`${label} 缺少响应体`);
  }
  const reader = response.body.getReader();
  const chunks = [];
  let total = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > MAX_API_BYTES) throw new Error(`${label} 响应超过安全上限`);
      chunks.push(Buffer.from(value));
    }
  } finally {
    try { reader.releaseLock(); } catch { /* 主校验结果优先。 */ }
  }
  try {
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(Buffer.concat(chunks)));
  } catch {
    throw new Error(`${label} 返回无效 JSON`);
  }
}

/** 使用 header 中的 token 读取固定 private Actions API endpoint。 */
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
    throw new Error("GitHub Actions API 请求失败");
  }
  if (!response || response.redirected || response.status < 200 || response.status >= 300) {
    try { await response?.body?.cancel(); } catch { /* HTTP 身份错误优先。 */ }
    throw new Error(`GitHub Actions API 返回 HTTP ${response?.status ?? "unknown"}`);
  }
  if (!/^application\/json(?:\s*;|$)/i.test(response.headers.get("content-type") ?? "")) {
    try { await response.body?.cancel(); } catch { /* Content-Type 错误优先。 */ }
    throw new Error("GitHub Actions API 返回错误 Content-Type");
  }
  return readBoundedJson(response, "GitHub Actions API");
}

/** 返回本次 exact-two smoke 允许存在的三个 artifact 名称。 */
export function expectedEvidenceArtifactNames(publicTag, sourceSha) {
  if (!/^v\d+\.\d+\.\d+$/.test(publicTag) || !/^[0-9a-f]{40}$/.test(sourceSha)) {
    throw new Error("evidence artifact tag/SHA identity 无效");
  }
  const prefix = `narrant-update-smoke-${publicTag}-${sourceSha}`;
  return [prefix, `${prefix}-darwin-aarch64`, `${prefix}-windows-x86_64`].sort();
}

/** 验证 evidence run 来自可信手动 smoke，或绑定 source tag/SHA 的自动 release。 */
export function validateEvidenceWorkflowRun(run, expected) {
  requireDatabaseId(expected.runId, "evidence run ID");
  const commonInvalid =
    !run ||
    run.id !== expected.runId ||
    run.status !== "completed" ||
    run.conclusion !== "success" ||
    run.run_attempt !== 1 ||
    run.repository?.full_name !== PRIVATE_REPOSITORY ||
    run.repository?.private !== true ||
    run.head_repository?.full_name !== PRIVATE_REPOSITORY ||
    run.head_repository?.private !== true ||
    !Array.isArray(run.pull_requests) ||
    run.pull_requests.length !== 0;
  const manualValid =
    run?.name === WORKFLOW_NAME && run.path === WORKFLOW_PATH && run.event === "workflow_dispatch" &&
    run.head_branch === expected.defaultBranch && run.head_sha === expected.workflowSha;
  const automaticValid =
    run?.name === RELEASE_WORKFLOW_NAME && run.path === RELEASE_WORKFLOW_PATH && run.event === "push" &&
    run.head_branch === expected.publicTag && run.head_sha === expected.sourceSha;
  if (commonInvalid || (!manualValid && !automaticValid)) {
    throw new Error("evidence run workflow/repository/ref identity 无效");
  }
  return automaticValid ? "release" : "smoke";
}

/** 验证手动 smoke 的三证据，或自动 release 的两构建加三证据 exact set。 */
export function validateEvidenceArtifacts(payload, expected, runKind = "smoke") {
  const smokeNames = expectedEvidenceArtifactNames(expected.publicTag, expected.sourceSha);
  const version = expected.publicTag.slice(1);
  const expectedNames = runKind === "release"
    ? [...smokeNames, `narrant-macos-${version}`, `narrant-windows-${version}`].sort()
    : smokeNames;
  const expectedBranch = runKind === "release" ? expected.publicTag : expected.defaultBranch;
  const expectedSha = runKind === "release" ? expected.sourceSha : expected.workflowSha;
  if (
    !payload ||
    payload.total_count !== expectedNames.length ||
    !Array.isArray(payload.artifacts) ||
    payload.artifacts.length !== expectedNames.length
  ) {
    throw new Error("evidence run artifact 数量不符合该 workflow 的 exact set");
  }
  const seen = new Set();
  for (const artifact of payload.artifacts) {
    requireDatabaseId(artifact?.id, "evidence artifact ID");
    const expectedArchiveUrl =
      `${API_ORIGIN}/repos/${PRIVATE_REPOSITORY}/actions/artifacts/${artifact.id}/zip`;
    if (
      !expectedNames.includes(artifact.name) ||
      seen.has(artifact.name) ||
      !Number.isSafeInteger(artifact.size_in_bytes) ||
      artifact.size_in_bytes <= 0 ||
      artifact.expired !== false ||
      artifact.archive_download_url !== expectedArchiveUrl ||
      !/^sha256:[0-9a-f]{64}$/.test(artifact.digest ?? "") ||
      artifact.workflow_run?.id !== expected.runId ||
      artifact.workflow_run?.head_branch !== expectedBranch ||
      artifact.workflow_run?.head_sha !== expectedSha
    ) {
      throw new Error("evidence artifact identity 无效");
    }
    seen.add(artifact.name);
  }
  if (expectedNames.some((name) => !seen.has(name))) {
    throw new Error("evidence artifact 名称集合不完整");
  }
  return payload.artifacts.find((artifact) => artifact.name === smokeNames[0]);
}

/** 通过只读 Actions API 绑定 evidence run 与该路径的精确 artifact 集合。 */
export async function verifyEvidenceRun(options, dependencies = {}) {
  const fetchImpl = dependencies.fetchImpl ?? globalThis.fetch;
  if (typeof fetchImpl !== "function") throw new Error("当前 Node 缺少 fetch");
  if (typeof options.token !== "string" || !options.token || options.token.trim() !== options.token) {
    throw new Error("验证 evidence run 需要有效 GITHUB_TOKEN");
  }
  const runId = requireDatabaseId(options.runId, "evidence run ID");
  const expected = { ...options, runId };
  const run = await githubApiJson(`/repos/${PRIVATE_REPOSITORY}/actions/runs/${runId}`, options.token, fetchImpl);
  const runKind = validateEvidenceWorkflowRun(
    run,
    expected,
  );
  const artifacts = await githubApiJson(
    `/repos/${PRIVATE_REPOSITORY}/actions/runs/${runId}/artifacts?per_page=100`,
    options.token,
    fetchImpl,
  );
  const aggregate = validateEvidenceArtifacts(artifacts, expected, runKind);
  return Object.freeze({ runId: run.id, workflowSha: run.head_sha, aggregateArtifactId: aggregate.id });
}

/** CLI 只输出公开 run/artifact database ID，不输出 token 或 API URL。 */
async function main() {
  const [runIdText, publicTag, sourceSha, defaultBranch, workflowSha] = process.argv.slice(2);
  const result = await verifyEvidenceRun({
    runId: Number(runIdText), publicTag, sourceSha, defaultBranch, workflowSha,
    token: process.env.GITHUB_TOKEN,
  });
  process.stdout.write(
    `已验证 evidence run ${result.runId}（aggregate artifact ${result.aggregateArtifactId}）。\n`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`错误：${error instanceof Error ? error.message : "evidence run 验证失败"}\n`);
    process.exitCode = 1;
  });
}

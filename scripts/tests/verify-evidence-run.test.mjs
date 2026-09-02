import assert from "node:assert/strict";
import test from "node:test";

import {
  expectedEvidenceArtifactNames,
  validateEvidenceArtifacts,
  validateEvidenceWorkflowRun,
  verifyEvidenceRun,
} from "../verify-evidence-run.mjs";

const expected = {
  runId: 42,
  publicTag: "v2.0.0",
  sourceSha: "a".repeat(40),
  defaultBranch: "main",
  workflowSha: "b".repeat(40),
};

/** 构造可信 private workflow run。 */
function runFixture() {
  return {
    id: 42,
    name: "Native application update smoke evidence",
    path: ".github/workflows/update-smoke.yml",
    event: "workflow_dispatch",
    status: "completed",
    conclusion: "success",
    head_branch: "main",
    head_sha: "b".repeat(40),
    run_attempt: 1,
    repository: { full_name: "XLCYun/narrant", private: true },
    head_repository: { full_name: "XLCYun/narrant", private: true },
    pull_requests: [],
  };
}

/** 构造 exact-three GitHub artifact API 响应。 */
function artifactsFixture() {
  const artifacts = expectedEvidenceArtifactNames(expected.publicTag, expected.sourceSha)
    .map((name, index) => ({
      id: index + 100,
      name,
      size_in_bytes: index + 10,
      expired: false,
      digest: `sha256:${String(index + 1).padStart(64, "0")}`,
      archive_download_url:
        `https://api.github.com/repos/XLCYun/narrant/actions/artifacts/${index + 100}/zip`,
      workflow_run: {
        id: expected.runId,
        head_branch: expected.defaultBranch,
        head_sha: expected.workflowSha,
      },
    }));
  return { total_count: 3, artifacts };
}

test("evidence run 绑定 private workflow、默认分支、可信 SHA 和首次 attempt", () => {
  assert.doesNotThrow(() => validateEvidenceWorkflowRun(runFixture(), expected));
  for (const mutate of [
    (run) => { run.path = ".github/workflows/other.yml"; },
    (run) => { run.event = "push"; },
    (run) => { run.conclusion = "failure"; },
    (run) => { run.head_branch = "feature"; },
    (run) => { run.head_sha = "c".repeat(40); },
    (run) => { run.run_attempt = 2; },
    (run) => { run.repository.full_name = "other/narrant"; },
    (run) => { run.head_repository.private = false; },
    (run) => { run.pull_requests = [{}]; },
  ]) {
    const copy = structuredClone(runFixture());
    mutate(copy);
    assert.throws(() => validateEvidenceWorkflowRun(copy, expected), /identity 无效/);
  }
});

test("evidence artifacts 精确绑定三个名称、digest、run、ref 与 archive URL", () => {
  const aggregate = validateEvidenceArtifacts(artifactsFixture(), expected);
  assert.equal(aggregate.name, `narrant-update-smoke-v2.0.0-${"a".repeat(40)}`);
  for (const mutate of [
    (payload) => { payload.total_count = 4; },
    (payload) => { payload.artifacts.pop(); },
    (payload) => { payload.artifacts[0].name = "unexpected"; },
    (payload) => { payload.artifacts[1].name = payload.artifacts[0].name; },
    (payload) => { payload.artifacts[0].expired = true; },
    (payload) => { payload.artifacts[0].digest = "sha256:bad"; },
    (payload) => { payload.artifacts[0].workflow_run.id = 43; },
    (payload) => { payload.artifacts[0].archive_download_url += "?redirect=1"; },
  ]) {
    const copy = structuredClone(artifactsFixture());
    mutate(copy);
    assert.throws(() => validateEvidenceArtifacts(copy, expected), /artifact/);
  }
});

test("Actions API 验证 token 只进入 header 且固定查询 run 与 artifact", async () => {
  const calls = [];
  const responses = [runFixture(), artifactsFixture()];
  const fetchImpl = async (url, options) => {
    calls.push({ url: String(url), options });
    return new Response(JSON.stringify(responses.shift()), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  const result = await verifyEvidenceRun({ ...expected, token: "secret-token" }, { fetchImpl });
  assert.deepEqual(result, { runId: 42, workflowSha: "b".repeat(40), aggregateArtifactId: 100 });
  assert.deepEqual(calls.map(({ url }) => url), [
    "https://api.github.com/repos/XLCYun/narrant/actions/runs/42",
    "https://api.github.com/repos/XLCYun/narrant/actions/runs/42/artifacts?per_page=100",
  ]);
  assert.equal(calls.every(({ url }) => !url.includes("secret-token")), true);
  assert.equal(calls.every(({ options }) => options.headers.Authorization === "Bearer secret-token"), true);
});

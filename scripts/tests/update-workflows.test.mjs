import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { URL } from "node:url";

const smokePath = new URL("../../.github/workflows/update-smoke.yml", import.meta.url);
const promotionPath = new URL("../../.github/workflows/promote-stable.yml", import.meta.url);

/** 要求 workflow 的每个 action reference 都固定为 40-hex commit。 */
function assertPinnedActions(text) {
  const uses = [...text.matchAll(/^\s*-?\s*uses:\s*([^\s#]+)/gm)].map((match) => match[1]);
  assert.ok(uses.length > 0);
  for (const reference of uses) assert.match(reference, /@[0-9a-f]{40}$/);
}

test("update-smoke 同时支持手动重试与 release 复用调用，matrix 精确覆盖两平台", async () => {
  const workflow = await readFile(smokePath, "utf8");
  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /workflow_call:/);
  assert.match(workflow, /CYUNLAB_RELEASE_TOKEN:\n\s+required: true/);
  assert.doesNotMatch(workflow, /release:\s*\n|types:\s*\[published\]/);
  assert.equal((workflow.slice(workflow.indexOf("\n  isolated-upgrade:")).match(/^\s*- runner:/gm) ?? []).length, 2);
  assert.match(workflow, /isolated-update-test/);
  assert.match(workflow, /test:upgrade:build/);
  assert.match(workflow, /--test-build-receipt/);
  assert.doesNotMatch(workflow, /--update-smoke|sudo|hosts/);
  assert.match(workflow, /runner: macos\n\s+target: darwin-aarch64/);
  assert.match(workflow, /runner: windows\n\s+target: windows-x86_64/);
  assert.doesNotMatch(workflow, /linux-|darwin-x86_64|windows-aarch64/);
  assert.match(workflow, /permissions:\n\s+contents: read/);
  assert.doesNotMatch(workflow, /id-token: write|attestations: write|attest-build-provenance/);
  assert.match(workflow, /CYUNLAB_RELEASE_TOKEN: \$\{\{ secrets\.CYUNLAB_RELEASE_TOKEN \}\}/);
  assert.match(workflow, /public-draft-assets\.mjs/);
  assert.match(workflow, /--draft-receipt aggregate-draft-receipt\.json/);
  assert.equal((workflow.match(/NARRANT_UPDATER_PUBLIC_KEY: \$\{\{ vars\.NARRANT_UPDATER_PUBLIC_KEY \}\}/g) ?? []).length, 3);
  assert.equal((workflow.match(/NARRANT_RELEASE_SIGNING_PUBLIC_KEY: \$\{\{ vars\.NARRANT_RELEASE_SIGNING_PUBLIC_KEY \}\}/g) ?? []).length, 3);
  assert.equal((workflow.match(/NARRANT_UPDATER_BRIDGE_TAG: \$\{\{ vars\.NARRANT_UPDATER_BRIDGE_TAG \}\}/g) ?? []).length, 3);
  assert.equal((workflow.match(/NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY:/g) ?? []).length, 3);
  assert.equal((workflow.match(/inputs\.mode == 'normal' && vars\.NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY \|\| ''/g) ?? []).length, 2);
  assert.match(workflow, /Prepare exact published previous Stable baseline[\s\S]*NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY: \$\{\{ vars\.NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY \}\}/);
  assert.doesNotMatch(workflow, /echo[^\r]*NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY/);
  assert.doesNotMatch(workflow, /environment: stable-promotion/);
  assertPinnedActions(workflow);
});

test("promotion 使用受保护 Environment、fixed ID 与可信 private evidence run 且不监听 public event", async () => {
  const workflow = await readFile(promotionPath, "utf8");
  assert.match(workflow, /workflow_dispatch:/);
  assert.doesNotMatch(workflow, /release:\s*\n|types:\s*\[published\]/);
  assert.match(workflow, /environment: stable-promotion/);
  assert.match(workflow, /group: narrant-stable-promotion\n\s+cancel-in-progress: false/);
  assert.match(workflow, /contents: read\n\s+actions: read\n\s+id-token: write/);
  assert.doesNotMatch(workflow, /attestations: read|gh attestation|attest-build-provenance/);
  assert.match(workflow, /verify-evidence-run\.mjs/);
  assert.match(workflow, /trusted_workflow_sha="\$\(git rev-parse HEAD\)"/);
  assert.match(workflow, /"\$EVIDENCE_RUN_ID" "\$PUBLIC_TAG" "\$SOURCE_SHA"/);
  assert.match(workflow, /"\$DEFAULT_BRANCH" "\$trusted_workflow_sha"/);
  assert.match(workflow, /run-id: \$\{\{ inputs\.evidence_run_id \}\}/);
  assert.match(workflow, /evidence_files=\(\n\s+smoke-evidence\/darwin-aarch64\.json\n\s+smoke-evidence\/darwin-aarch64\.json\.sha256\n\s+smoke-evidence\/windows-x86_64\.json\n\s+smoke-evidence\/windows-x86_64\.json\.sha256/);
  assert.match(workflow, /for evidence_file in "\$\{evidence_files\[@\]\}"; do/);
  assert.match(workflow, /shasum -a 256 --check darwin-aarch64\.json\.sha256 windows-x86_64\.json\.sha256/);
  assert.match(workflow, /promote-stable-release\.mjs/);
  assert.match(workflow, /CYUNLAB_RELEASE_TOKEN: \$\{\{ secrets\.CYUNLAB_RELEASE_TOKEN \}\}/);
  assert.match(workflow, /NARRANT_RELEASE_SIGNING_PUBLIC_KEY: \$\{\{ vars\.NARRANT_RELEASE_SIGNING_PUBLIC_KEY \}\}/);
  assert.match(workflow, /NARRANT_UPDATER_PUBLIC_KEY: \$\{\{ vars\.NARRANT_UPDATER_PUBLIC_KEY \}\}/);
  assert.match(workflow, /NARRANT_UPDATER_BRIDGE_TAG: \$\{\{ vars\.NARRANT_UPDATER_BRIDGE_TAG \}\}/);
  assert.match(workflow, /NARRANT_FIRST_UPDATER_TAG: \$\{\{ vars\.NARRANT_OSS_BOOTSTRAP_TAG \}\}/);
  assert.match(workflow, /ALIBABA_CLOUD_OIDC_PROVIDER_ARN/);
  assert.match(workflow, /ALIBABA_CLOUD_ROLE_ARN/);
  assert.match(workflow, /OSS_BUCKET: \$\{\{ vars\.OSS_BUCKET \}\}/);
  assert.match(workflow, /OSS_REGION: \$\{\{ vars\.OSS_REGION \}\}/);
  assert.match(workflow, /configure-aliyun-credentials-action@[0-9a-f]{40}/);
  assert.match(workflow, /bash scripts\/install-ci-ossutil\.sh/);
  assert.match(workflow, /NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY: \$\{\{ vars\.NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY \}\}/);
  assert.match(workflow, /cyunlab\/narrant/);
  assert.match(workflow, /XLCYun\/narrant|private source/);
  assertPinnedActions(workflow);
});

test("分支选择目标或同源基线不会放宽正式升级的双平台与上一 Stable 约束", async () => {
  const workflow = await readFile(smokePath, "utf8");
  const preflight = workflow.split("\n  preflight:")[1].split("\n  isolated-upgrade:")[0];
  const formal = workflow.split("\n  isolated-upgrade:")[1];
  assert.match(preflight, /inputs\.preflight_target/);
  assert.match(preflight, /inputs\.preflight_baseline/);
  assert.doesNotMatch(formal, /inputs\.preflight_target|inputs\.preflight_baseline/);
  assert.match(preflight, /narrant-update-preflight/);
  assert.match(preflight, /update-test-observation\/failure\.json/);
  assert.doesNotMatch(formal, /continue-on-error: true/);
  assert.match(formal, /--previous-source-sha/);
  assert.equal((formal.match(/^\s*- runner:/gm) ?? []).length, 2);
  const runner = await readFile(new URL("../../scripts/run-isolated-update-test.mjs", import.meta.url), "utf8");
  assert.match(runner, /same-source-version-regression/);
  assert.match(runner, /productionArtifactsVerified: false/);
});

test("新 workflows 不授予 contents write，也不把 release token 放进命令行", async () => {
  const workflows = `${await readFile(smokePath, "utf8")}\n${await readFile(promotionPath, "utf8")}`;
  assert.doesNotMatch(workflows, /contents:\s*write/);
  assert.doesNotMatch(workflows, /node scripts\/[^\n]*\$CYUNLAB_RELEASE_TOKEN/);
  assert.doesNotMatch(workflows, /https?:\/\/[^\s]*\$\{\{\s*secrets\./);
});

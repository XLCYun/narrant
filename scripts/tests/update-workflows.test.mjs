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

test("update-smoke 只有手动可信输入且 matrix 精确覆盖 macOS15 arm64/Windows x64", async () => {
  const workflow = await readFile(smokePath, "utf8");
  assert.match(workflow, /workflow_dispatch:/);
  assert.doesNotMatch(workflow, /release:\s*\n|types:\s*\[published\]/);
  assert.equal((workflow.match(/^\s*- runner:/gm) ?? []).length, 2);
  assert.match(workflow, /runner: macos-15\n\s+target: darwin-aarch64/);
  assert.match(workflow, /runner: windows-2025\n\s+target: windows-x86_64/);
  assert.doesNotMatch(workflow, /linux-|darwin-x86_64|windows-aarch64/);
  assert.match(workflow, /permissions:\n\s+contents: read/);
  assert.match(workflow, /id-token: write\n\s+attestations: write/);
  assert.match(workflow, /attest-build-provenance@[0-9a-f]{40}/);
  assert.match(workflow, /CYUNLAB_RELEASE_TOKEN: \$\{\{ secrets\.CYUNLAB_RELEASE_TOKEN \}\}/);
  assert.match(workflow, /public-draft-assets\.mjs/);
  assert.match(workflow, /--draft-receipt aggregate-draft-receipt\.json/);
  assert.equal((workflow.match(/NARRANT_UPDATER_PUBLIC_KEY: \$\{\{ vars\.NARRANT_UPDATER_PUBLIC_KEY \}\}/g) ?? []).length, 3);
  assert.equal((workflow.match(/NARRANT_RELEASE_SIGNING_PUBLIC_KEY: \$\{\{ vars\.NARRANT_RELEASE_SIGNING_PUBLIC_KEY \}\}/g) ?? []).length, 4);
  assert.equal((workflow.match(/NARRANT_UPDATER_BRIDGE_TAG: \$\{\{ vars\.NARRANT_UPDATER_BRIDGE_TAG \}\}/g) ?? []).length, 3);
  assert.doesNotMatch(workflow, /environment: stable-promotion/);
  assertPinnedActions(workflow);
});

test("promotion 使用受保护 Environment、fixed ID、private evidence attestation 且不监听 public event", async () => {
  const workflow = await readFile(promotionPath, "utf8");
  assert.match(workflow, /workflow_dispatch:/);
  assert.doesNotMatch(workflow, /release:\s*\n|types:\s*\[published\]/);
  assert.match(workflow, /environment: stable-promotion/);
  assert.match(workflow, /group: narrant-stable-promotion\n\s+cancel-in-progress: false/);
  assert.match(workflow, /contents: read\n\s+actions: read\n\s+attestations: read/);
  assert.match(workflow, /run-id: \$\{\{ inputs\.evidence_run_id \}\}/);
  assert.match(workflow, /evidence_files=\(\n\s+smoke-evidence\/darwin-aarch64\.json\n\s+smoke-evidence\/darwin-aarch64\.json\.sha256\n\s+smoke-evidence\/windows-x86_64\.json\n\s+smoke-evidence\/windows-x86_64\.json\.sha256/);
  assert.match(workflow, /for evidence_file in "\$\{evidence_files\[@\]\}"; do/);
  assert.match(workflow, /gh attestation verify "\$evidence_file"/);
  assert.doesNotMatch(workflow, /gh attestation verify smoke-evidence\/\*/);
  assert.match(workflow, /--signer-workflow "github\.com\/\$GITHUB_REPOSITORY\/\.github\/workflows\/update-smoke\.yml"/);
  assert.doesNotMatch(workflow, /--signer-workflow "https?:\/\//);
  assert.match(workflow, /--source-ref "refs\/heads\/\$DEFAULT_BRANCH"/);
  assert.match(workflow, /--deny-self-hosted-runners/);
  assert.match(workflow, /promote-stable-release\.mjs/);
  assert.match(workflow, /CYUNLAB_RELEASE_TOKEN: \$\{\{ secrets\.CYUNLAB_RELEASE_TOKEN \}\}/);
  assert.match(workflow, /NARRANT_RELEASE_SIGNING_PUBLIC_KEY: \$\{\{ vars\.NARRANT_RELEASE_SIGNING_PUBLIC_KEY \}\}/);
  assert.match(workflow, /NARRANT_UPDATER_PUBLIC_KEY: \$\{\{ vars\.NARRANT_UPDATER_PUBLIC_KEY \}\}/);
  assert.match(workflow, /NARRANT_UPDATER_BRIDGE_TAG: \$\{\{ vars\.NARRANT_UPDATER_BRIDGE_TAG \}\}/);
  assert.match(workflow, /NARRANT_FIRST_UPDATER_TAG: \$\{\{ vars\.NARRANT_FIRST_UPDATER_TAG \}\}/);
  assert.match(workflow, /cyunlab\/narrant/);
  assert.match(workflow, /XLCYun\/narrant|private source/);
  assertPinnedActions(workflow);
});

test("新 workflows 不授予 contents write，也不把 release token 放进命令行", async () => {
  const workflows = `${await readFile(smokePath, "utf8")}\n${await readFile(promotionPath, "utf8")}`;
  assert.doesNotMatch(workflows, /contents:\s*write/);
  assert.doesNotMatch(workflows, /node scripts\/[^\n]*\$CYUNLAB_RELEASE_TOKEN/);
  assert.doesNotMatch(workflows, /https?:\/\/[^\s]*\$\{\{\s*secrets\./);
});

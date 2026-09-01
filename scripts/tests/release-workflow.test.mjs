import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { URL } from "node:url";

const workflow = readFileSync(new URL("../../.github/workflows/release.yml", import.meta.url), "utf8");
const endpoint = "https://github.com/cyunlab/narrant/releases/latest/download/latest.json";

/** 统计工作流中固定文本出现次数，避免只配置了单个平台。 */
function occurrences(text) {
  return workflow.split(text).length - 1;
}

test("macOS 与 Windows 构建都注入并预验 updater 私钥、密码、固定 endpoint 和公钥", () => {
  assert.equal(occurrences("TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}"), 2);
  assert.equal(occurrences("TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}"), 2);
  assert.equal(occurrences(`NARRANT_UPDATER_ENDPOINT: ${endpoint}`), 2);
  assert.equal(occurrences("NARRANT_UPDATER_PUBLIC_KEY: ${{ vars.NARRANT_UPDATER_PUBLIC_KEY }}"), 3);
  assert.equal(occurrences("name: 在构建前校验 updater trust 配置"), 2);
  assert.ok(workflow.indexOf("在构建前校验 updater trust 配置") < workflow.indexOf("pnpm tauri build"));
});

test("macOS 从最终签名公证 app 重建并签署 updater archive，DMG 独立验证", () => {
  const recursiveSign = workflow.indexOf("find \"$app_path/Contents\" -type f");
  const stapleApp = workflow.indexOf("xcrun stapler staple \"$app_path\"");
  const rebuildArchive = workflow.indexOf("tar -czf \"$updater_archive\"");
  const updaterSign = workflow.indexOf("pnpm tauri signer sign \"$updater_archive\"");
  const createDmg = workflow.indexOf("hdiutil create -volname Narrant");
  const stapleDmg = workflow.indexOf("xcrun stapler staple \"$dmg_path\"");
  assert.ok(recursiveSign > 0);
  assert.ok(recursiveSign < stapleApp);
  assert.ok(stapleApp < rebuildArchive);
  assert.ok(rebuildArchive < updaterSign);
  assert.ok(updaterSign < createDmg);
  assert.ok(createDmg < stapleDmg);
  assert.match(workflow, /narrant-darwin-aarch64-updater\.app\.tar\.gz\.sig/);
});

test("Windows 收集 current-user NSIS updater exe 与强制签名", () => {
  assert.match(workflow, /构建带 updater 签名的 current-user Windows x64 NSIS/);
  assert.match(workflow, /bundle\/nsis\/\*-setup\.exe\.sig/);
  assert.match(workflow, /narrant-windows-x86_64-updater\.exe/);
  assert.match(workflow, /narrant-windows-x86_64-updater\.exe\.sig/);
});

test("公开 Draft 使用跨仓库 token，且完整本地预验严格早于远端协调", () => {
  assert.match(workflow, /GH_TOKEN: \$\{\{ secrets\.CYUNLAB_RELEASE_TOKEN \}\}/);
  assert.match(workflow, /TARGET_REPOSITORY: cyunlab\/narrant/);
  const prepare = workflow.indexOf("node scripts/prepare-draft-assets.mjs");
  const reconcile = workflow.indexOf("node scripts/reconcile-draft-release.mjs");
  assert.ok(prepare > 0);
  assert.ok(prepare < reconcile);
  assert.match(workflow, /release-assets\/\*/);
});

test("Task 5 工作流不包含发布、latest promotion 或 release.published 触发器", () => {
  assert.doesNotMatch(workflow, /gh release publish/i);
  assert.doesNotMatch(workflow, /--latest\b/);
  assert.doesNotMatch(workflow, /draft\s*[:=]\s*false/i);
  assert.doesNotMatch(workflow, /release\.published/i);
});

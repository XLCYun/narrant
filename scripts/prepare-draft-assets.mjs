#!/usr/bin/env node

import { readFileSync } from "node:fs";
import process from "node:process";

import { prepareDraftAssetDirectory } from "./release-assets.mjs";

/** 从受控环境读取发布输入并准备完整 Draft 目录。 */
function main() {
  const [directory, notesFile] = process.argv.slice(2);
  if (!directory || !notesFile) {
    throw new Error("用法：node scripts/prepare-draft-assets.mjs <assets-directory> <notes-file>");
  }
  const result = prepareDraftAssetDirectory({
    directory,
    tag: process.env.RELEASE_TAG,
    version: process.env.RELEASE_VERSION,
    notes: readFileSync(notesFile, "utf8"),
    releaseSigningPublicKey: process.env.NARRANT_RELEASE_SIGNING_PUBLIC_KEY,
    candidateUpdaterPublicKey: process.env.NARRANT_UPDATER_PUBLIC_KEY,
    bridgeTag: process.env.NARRANT_UPDATER_BRIDGE_TAG,
    privateRepository: process.env.GITHUB_REPOSITORY,
  });
  process.stdout.write(`已验证 ${result.names.length} 个 Draft 资产。\n`);
}

try {
  main();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`错误：${message}\n`);
  process.exitCode = 1;
}

#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import process from "node:process";

import { VERSION_PATTERN } from "./release-version.mjs";

/** 运行只读 Git 查询并保留提交主题原始顺序。 */
function git(arguments_) {
  return execFileSync("git", arguments_, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

/** 生成不含私有 commit 身份字段的公开 Release Notes。 */
function main() {
  const [tag, output] = process.argv.slice(2);
  const version = tag?.startsWith("v") ? tag.slice(1) : "";
  if (!VERSION_PATTERN.test(version) || !output) {
    throw new Error("用法：node scripts/generate-public-release-notes.mjs <vVERSION> <output>");
  }
  const previousTag = git(["tag", "--list", "v*", "--sort=-v:refname"])
    .split("\n")
    .find((candidate) => candidate !== tag && VERSION_PATTERN.test(candidate.slice(1)));
  const range = previousTag ? `${previousTag}..${tag}` : tag;
  const subjects = git(["log", "--pretty=%s", range]);
  const changes = subjects ? subjects.split("\n").map((subject) => `- ${subject}`).join("\n") : "- 本次发布没有用户可见变动。";
  const notes = [
    `## Narrant ${version}`,
    "",
    "### 版本变动",
    changes,
    "",
    "> Windows x64 安装包当前未进行 Authenticode 签名。",
    "",
  ].join("\n");
  writeFileSync(output, notes);
}

try {
  main();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`错误：${message}\n`);
  process.exitCode = 1;
}

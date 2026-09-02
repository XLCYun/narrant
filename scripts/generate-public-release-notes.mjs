#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import process from "node:process";
import { pathToFileURL } from "node:url";

import { assertPublicReleaseMetadata } from "./release-assets.mjs";
import { VERSION_PATTERN } from "./release-version.mjs";

/** 运行只读 Git 查询并保留提交主题原始顺序。 */
function git(arguments_) {
  return execFileSync("git", arguments_, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

/** 生成、校验并写入公开 Release Notes；测试可在 git/write seam 注入内存 adapter。 */
export function generatePublicReleaseNotes(options, dependencies = {}) {
  const { tag, output, privateRepository } = options;
  const runGit = dependencies.git ?? git;
  const writeFile = dependencies.writeFile ?? writeFileSync;
  const version = tag?.startsWith("v") ? tag.slice(1) : "";
  if (!VERSION_PATTERN.test(version) || !output) {
    throw new Error("用法：node scripts/generate-public-release-notes.mjs <vVERSION> <output>");
  }
  const previousTag = runGit(["tag", "--list", "v*", "--sort=-v:refname"])
    .split("\n")
    .find((candidate) => candidate !== tag && VERSION_PATTERN.test(candidate.slice(1)));
  const range = previousTag ? `${previousTag}..${tag}` : tag;
  const subjects = runGit(["log", "--pretty=%s", range]);
  const changes = subjects ? subjects.split("\n").map((subject) => `- ${subject}`).join("\n") : "- 本次发布没有用户可见变动。";
  const title = `Narrant ${version}`;
  const notes = [
    `## ${title}`,
    "",
    "### 版本变动",
    changes,
    "",
    "> Windows x64 安装包当前未进行 Authenticode 签名。",
    "",
  ].join("\n");
  assertPublicReleaseMetadata({ title, notes, privateRepository });
  writeFile(output, notes);
  return { title, notes, range };
}

/** 从 Actions 环境读取发布身份并调用公开 Notes 生成模块。 */
function main() {
  const [tag, output] = process.argv.slice(2);
  generatePublicReleaseNotes({
    tag,
    output,
    privateRepository: process.env.GITHUB_REPOSITORY,
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`错误：${message}\n`);
    process.exitCode = 1;
  }
}

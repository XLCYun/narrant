#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import process from "node:process";
import {
  assertDeclaredVersion,
  compareVersions,
  readVersionFileContents,
  VERSION_PATTERN,
} from "./release-version.mjs";

/** 使用当前仓库执行只读 Git 查询。 */
function git(repository, arguments_) {
  return execFileSync("git", arguments_, {
    cwd: repository,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

/** 校验 Actions 收到的标签，并只向标准输出写入版本号。 */
function checkReleaseVersion() {
  const tag = process.argv[2] ?? "";
  const version = tag.startsWith("v") ? tag.slice(1) : "";
  if (!VERSION_PATTERN.test(version)) {
    throw new Error("仅支持 vMAJOR.MINOR.PATCH 格式的稳定版本标签。");
  }

  const repository = git(process.cwd(), ["rev-parse", "--show-toplevel"]);
  const latestOtherTag = git(repository, ["tag", "--list", "v*", "--sort=-v:refname"])
    .split("\n")
    .find((candidate) => candidate !== tag && VERSION_PATTERN.test(candidate.slice(1)));
  if (latestOtherTag && compareVersions(version, latestOtherTag.slice(1)) <= 0) {
    throw new Error(`${tag} 必须高于已有的最新稳定标签 ${latestOtherTag}。`);
  }

  assertDeclaredVersion(readVersionFileContents(repository), version);
  process.stdout.write(`${version}\n`);
}

try {
  checkReleaseVersion();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`错误：${message}\n`);
  process.exitCode = 1;
}

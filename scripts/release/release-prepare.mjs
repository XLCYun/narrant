#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import process from "node:process";
import {
  compareVersions,
  readCurrentVersion,
  readVersionFileContents,
  VERSION_FILES,
  VERSION_PATTERN,
} from "./release-version.mjs";

/** 使用当前仓库执行 Git，并返回去除末尾换行的输出。 */
function git(repository, arguments_) {
  return execFileSync("git", arguments_, {
    cwd: repository,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

/** 只替换 JSON 文件中的顶层版本字段，保留原有排版。 */
function updateJsonVersion(content, currentVersion, nextVersion) {
  return content.replace(
    new RegExp(`("version"\\s*:\\s*")${currentVersion.replaceAll(".", "\\.")}(")`),
    `$1${nextVersion}$2`,
  );
}

/** 只替换 TOML 指定表内的版本字段。 */
function updateTomlTableVersion(content, tableName, currentVersion, nextVersion) {
  const tablePattern = new RegExp(`(\\[${tableName}\\][\\s\\S]*?^version\\s*=\\s*")${currentVersion.replaceAll(".", "\\.")}(")`, "m");
  return content.replace(tablePattern, `$1${nextVersion}$2`);
}

/** 归一化 Narrant 根包版本，以验证 Cargo 没有改变任何其他锁定内容。 */
function normalizeLockVersion(content) {
  const packagePattern = /(\[\[package\]\][\s\S]*?^name\s*=\s*"narrant"\s*$[\s\S]*?^version\s*=\s*")[^"]+(")/m;
  const normalized = content.replace(packagePattern, "$1<version>$2");
  if (normalized === content) {
    throw new Error("无法在 Cargo.lock 中定位 Narrant 根包版本。");
  }
  return normalized;
}

/** 准备版本提交和 annotated tag，但把推送动作留给发布者确认。 */
function prepareRelease() {
  const nextVersion = process.argv[2];
  if (!VERSION_PATTERN.test(nextVersion ?? "")) {
    throw new Error("用法：pnpm release:prepare <MAJOR.MINOR.PATCH>");
  }

  const repository = git(process.cwd(), ["rev-parse", "--show-toplevel"]);
  if (git(repository, ["status", "--porcelain"]) !== "") {
    throw new Error("工作区必须干净，发布准备不会覆盖现有修改。");
  }

  const contents = readVersionFileContents(repository);
  const currentVersion = readCurrentVersion(contents);
  if (!VERSION_PATTERN.test(currentVersion) || compareVersions(nextVersion, currentVersion) <= 0) {
    throw new Error(`新版本 ${nextVersion} 必须高于当前稳定版本 ${currentVersion}。`);
  }

  const latestStableTag = git(repository, ["tag", "--list", "v*", "--sort=-v:refname"])
    .split("\n")
    .find((candidate) => VERSION_PATTERN.test(candidate.slice(1)));
  if (
    latestStableTag &&
    compareVersions(nextVersion, latestStableTag.slice(1)) <= 0
  ) {
    throw new Error(`新版本 ${nextVersion} 必须高于最新稳定标签 ${latestStableTag}。`);
  }

  const tag = `v${nextVersion}`;
  try {
    git(repository, ["rev-parse", "--verify", `refs/tags/${tag}`]);
    throw new Error(`标签 ${tag} 已存在。`);
  } catch (error) {
    if (error instanceof Error && error.message === `标签 ${tag} 已存在。`) {
      throw error;
    }
  }

  const updates = new Map([
    ["package.json", updateJsonVersion(contents.get("package.json"), currentVersion, nextVersion)],
    [
      "src-tauri/tauri.conf.json",
      updateJsonVersion(contents.get("src-tauri/tauri.conf.json"), currentVersion, nextVersion),
    ],
    [
      "src-tauri/Cargo.toml",
      updateTomlTableVersion(
        contents.get("src-tauri/Cargo.toml"),
        "package",
        currentVersion,
        nextVersion,
      ),
    ],
  ]);

  if ([...updates].some(([path, content]) => content === contents.get(path))) {
    throw new Error("无法定位全部版本字段，未修改任何文件。");
  }

  const originalHead = git(repository, ["rev-parse", "HEAD"]);
  const branchReference = git(repository, ["symbolic-ref", "HEAD"]);
  let commitCreated = false;
  try {
    for (const [path, content] of updates) {
      writeFileSync(resolve(repository, path), content);
    }
    // Cargo 拥有锁文件格式；metadata 同步根包版本，随后严格拒绝任何依赖锁变化。
    execFileSync(
      "cargo",
      [
        "metadata",
        "--manifest-path",
        resolve(repository, "src-tauri/Cargo.toml"),
        "--format-version",
        "1",
      ],
      { cwd: repository, stdio: ["ignore", "ignore", "pipe"] },
    );
    const generatedContents = readVersionFileContents(repository);
    const generatedVersion = readCurrentVersion(generatedContents);
    if (generatedVersion !== nextVersion) {
      throw new Error(`Cargo 生成锁文件后版本仍为 ${generatedVersion}，期望 ${nextVersion}。`);
    }
    if (
      normalizeLockVersion(generatedContents.get("src-tauri/Cargo.lock")) !==
      normalizeLockVersion(contents.get("src-tauri/Cargo.lock"))
    ) {
      throw new Error("Cargo metadata 改变了版本以外的锁定内容，发布准备已回滚。");
    }
    git(repository, ["add", "--", ...VERSION_FILES]);
    git(repository, ["commit", "-m", `chore(release): ${tag}`]);
    commitCreated = true;
    git(repository, ["tag", "-a", tag, "-m", tag]);
  } catch (error) {
    // 只撤销本脚本从干净工作区产生的提交和文件修改，不触碰其他路径。
    if (commitCreated) {
      const failedHead = git(repository, ["rev-parse", "HEAD"]);
      git(repository, ["update-ref", branchReference, originalHead, failedHead]);
    }
    git(repository, ["update-ref", "-d", `refs/tags/${tag}`]);
    git(repository, ["restore", "--source", originalHead, "--staged", "--worktree", "--", ...VERSION_FILES]);
    throw error;
  }
  process.stdout.write(`已创建 ${tag}。检查提交后运行：git push origin HEAD ${tag}\n`);
}

try {
  prepareRelease();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`错误：${message}\n`);
  process.exitCode = 1;
}

import assert from "node:assert/strict";
import test from "node:test";

import { generatePublicReleaseNotes } from "../generate-public-release-notes.mjs";

/** 创建只返回 tag 列表与 commit subjects 的内存 git/write adapters。 */
function releaseNotesAdapters(subjects) {
  const gitCalls = [];
  const writes = [];
  return {
    gitCalls,
    writes,
    dependencies: {
      git(arguments_) {
        gitCalls.push(arguments_);
        if (arguments_[0] === "tag") return "v2.0.0\nv1.9.0";
        if (arguments_[0] === "log") return subjects;
        throw new Error(`未预期的 git 调用：${arguments_.join(" ")}`);
      },
      writeFile(path, contents) {
        writes.push({ path, contents });
      },
    },
  };
}

/** 返回固定 tag/output 的 generator 输入。 */
function generatorOptions() {
  return {
    tag: "v2.0.0",
    output: "/tmp/release-notes.md",
  };
}

test("公开 conventional subjects 保持原顺序和既有 Notes 格式", () => {
  const subjects = [
    "fix: 修复公开 issue https://github.com/XLCYun/narrant/issues/42",
    "feat: 增加自动更新提示",
  ].join("\n");
  const adapters = releaseNotesAdapters(subjects);
  const result = generatePublicReleaseNotes(generatorOptions(), adapters.dependencies);
  const expected = [
    "## Narrant 2.0.0",
    "",
    "### 版本变动",
    "- fix: 修复公开 issue https://github.com/XLCYun/narrant/issues/42",
    "- feat: 增加自动更新提示",
    "",
    "> Windows x64 安装包当前未进行 Authenticode 签名。",
    "",
  ].join("\n");

  assert.deepEqual(result, {
    title: "Narrant 2.0.0",
    notes: expected,
    range: "v1.9.0..v2.0.0",
  });
  assert.deepEqual(adapters.writes, [{ path: "/tmp/release-notes.md", contents: expected }]);
  assert.deepEqual(adapters.gitCalls.at(-1), ["log", "--pretty=%s", "v1.9.0..v2.0.0"]);
});

test("无用户可见 subject 时保留既有空变动文案", () => {
  const adapters = releaseNotesAdapters("");
  const result = generatePublicReleaseNotes(generatorOptions(), adapters.dependencies);
  assert.match(result.notes, /- 本次发布没有用户可见变动。/);
  assert.equal(adapters.writes.length, 1);
});

test("开源发布说明保留提交引用和第三方链接", () => {
  const subject = "fix: deadbeef https://github.com/XLCYun/narrant/commit/deadbeef https://github.com/tauri-apps/tauri/issues/1";
  const adapters = releaseNotesAdapters(subject);
  const result = generatePublicReleaseNotes(generatorOptions(), adapters.dependencies);
  assert.ok(result.notes.includes(subject));
  assert.equal(adapters.writes.length, 1);
});

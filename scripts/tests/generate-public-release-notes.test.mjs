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

/** 返回固定 tag/output/private-repository 的 generator 输入。 */
function generatorOptions() {
  return {
    tag: "v2.0.0",
    output: "/tmp/release-notes.md",
    privateRepository: "cyunlab/narrant-private",
  };
}

test("公开 conventional subjects 保持原顺序和既有 Notes 格式", () => {
  const subjects = [
    "fix: 修复公开 issue https://github.com/cyunlab/narrant/issues/42",
    "feat: 增加自动更新提示",
  ].join("\n");
  const adapters = releaseNotesAdapters(subjects);
  const result = generatePublicReleaseNotes(generatorOptions(), adapters.dependencies);
  const expected = [
    "## Narrant 2.0.0",
    "",
    "### 版本变动",
    "- fix: 修复公开 issue https://github.com/cyunlab/narrant/issues/42",
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

test("private identity、SHA、issue/PR URL 与 GFM autolink 均在写文件前 fail closed", async (context) => {
  const fixtures = [
    ["private repository identity", "fix: cyunlab/narrant-private", /私有仓库身份/],
    ["private repository URL", "fix: https://github.com/cyunlab/narrant-private/issues/7", /私有仓库身份/],
    ["private issue URL", "fix: https://github.com/other/private/issues/8", /未允许 URL/],
    ["private PR Markdown", "fix: [内部 PR](https://github.com/other/private/pull/9)", /未允许 URL/],
    ["full SHA", `fix: ${"a".repeat(40)}`, /完整 Git SHA/],
    ["GFM bare www", "fix: www.internal.example/runbook", /未允许 URL/],
    ["GFM email autolink", "fix: private@example.com", /未允许 URL/],
  ];
  for (const [name, subject, pattern] of fixtures) {
    await context.test(name, () => {
      const adapters = releaseNotesAdapters(subject);
      assert.throws(
        () => generatePublicReleaseNotes(generatorOptions(), adapters.dependencies),
        pattern,
      );
      assert.equal(adapters.writes.length, 0);
    });
  }
});

test("缺少 GITHUB_REPOSITORY 身份时不会写公开 Notes", () => {
  const adapters = releaseNotesAdapters("fix: safe subject");
  assert.throws(
    () => generatePublicReleaseNotes(
      { ...generatorOptions(), privateRepository: undefined },
      adapters.dependencies,
    ),
    /缺少私有仓库身份/,
  );
  assert.equal(adapters.writes.length, 0);
});

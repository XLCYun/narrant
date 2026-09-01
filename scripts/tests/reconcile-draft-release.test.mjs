import assert from "node:assert/strict";
import test from "node:test";

import { releaseAssetNames } from "../release-assets.mjs";
import { reconcileDraftRelease } from "../reconcile-draft-release.mjs";

/** 返回当前版本八个经过本地预验的虚拟上传路径。 */
function artifactPaths(version = "2.3.4") {
  const names = releaseAssetNames(version);
  return [
    names.installer.darwin,
    names.installer.windows,
    names.updater.darwin,
    `${names.updater.darwin}.sig`,
    names.updater.windows,
    `${names.updater.windows}.sig`,
    names.manifest,
    names.checksums,
  ].map((name) => `/tmp/release-assets/${name}`);
}

/** 构造协调模块的固定合法输入。 */
function options() {
  return {
    repository: "cyunlab/narrant",
    tag: "v2.3.4",
    version: "2.3.4",
    title: "Narrant 2.3.4",
    notesFile: "/tmp/release-notes.md",
    artifacts: artifactPaths(),
  };
}

/** 创建有状态的内存 gh adapter，覆盖查询、创建、编辑、删除和上传语义。 */
function createGhAdapter(initialRelease, hooks = {}) {
  let release = initialRelease ? JSON.parse(JSON.stringify(initialRelease)) : undefined;
  let nextAssetId = 100;
  const calls = [];
  const runGh = async (arguments_) => {
    calls.push(arguments_);
    if (arguments_[0] === "api" && arguments_[1]?.startsWith("repos/") && arguments_[1]?.includes("/releases/tags/")) {
      if (hooks.lookupError) {
        return { exitCode: 1, stdout: "", stderr: hooks.lookupError };
      }
      return release
        ? { exitCode: 0, stdout: JSON.stringify(release), stderr: "" }
        : { exitCode: 1, stdout: "", stderr: "gh: Not Found (HTTP 404)" };
    }
    if (arguments_[0] === "api" && arguments_.includes("DELETE")) {
      const id = Number(arguments_.at(-1).split("/").at(-1));
      release.assets = release.assets.filter((asset) => asset.id !== id);
      return { exitCode: 0, stdout: "", stderr: "" };
    }
    if (arguments_[0] === "release" && arguments_[1] === "create") {
      release = { id: 42, draft: true, tag_name: arguments_[2], assets: [] };
      return { exitCode: 0, stdout: "", stderr: "" };
    }
    if (arguments_[0] === "release" && arguments_[1] === "edit") {
      return { exitCode: 0, stdout: "", stderr: "" };
    }
    if (arguments_[0] === "release" && arguments_[1] === "upload") {
      const repositoryFlag = arguments_.indexOf("--repo");
      const paths = arguments_.slice(3, repositoryFlag);
      release.assets = paths.map((path) => ({ id: nextAssetId += 1, name: path.split("/").at(-1) }));
      if (hooks.afterUpload) {
        hooks.afterUpload(release);
      }
      return { exitCode: 0, stdout: "", stderr: "" };
    }
    throw new Error(`测试 gh adapter 收到未知命令：${arguments_.join(" ")}`);
  };
  return { runGh, calls, release: () => release };
}

test("不存在时创建 Draft、上传八文件并通过 API 回读精确集合", async () => {
  const gh = createGhAdapter();
  const result = await reconcileDraftRelease(options(), { runGh: gh.runGh });

  assert.deepEqual(result, { outcome: "created", releaseId: 42 });
  assert.deepEqual(
    gh.release().assets.map((asset) => asset.name).sort(),
    artifactPaths().map((path) => path.split("/").at(-1)).sort(),
  );
  assert.equal(gh.calls.some((call) => call[1] === "create" && call.includes("--draft")), true);
});

test("已有 Draft 被完整替换，失败后的同 tag 重试保持幂等", async () => {
  const gh = createGhAdapter({
    id: 42,
    draft: true,
    tag_name: "v2.3.4",
    assets: [{ id: 7, name: "partial.bin" }, { id: 8, name: "latest.json" }],
  });
  const first = await reconcileDraftRelease(options(), { runGh: gh.runGh });
  const second = await reconcileDraftRelease(options(), { runGh: gh.runGh });

  assert.equal(first.outcome, "updated");
  assert.equal(second.outcome, "updated");
  assert.equal(gh.release().assets.length, 8);
  const deleteCalls = gh.calls.filter((call) => call[0] === "api" && call.includes("DELETE"));
  assert.equal(deleteCalls.length, 10);
  assert.equal(gh.calls.some((call) => call[1] === "edit"), true);
});

test("非 404 的 API 查找错误不会被误判为新 Draft", async () => {
  const gh = createGhAdapter(undefined, { lookupError: "gh: rate limited (HTTP 403)" });
  await assert.rejects(
    reconcileDraftRelease(options(), { runGh: gh.runGh }),
    /无法检查 Release.*403/,
  );
  assert.equal(gh.calls.some((call) => call[1] === "create"), false);
});

test("发现 published Release 时拒绝任何修改和上传", async () => {
  const gh = createGhAdapter({
    id: 42,
    draft: false,
    tag_name: "v2.3.4",
    assets: [{ id: 7, name: "published.exe" }],
  });
  await assert.rejects(
    reconcileDraftRelease(options(), { runGh: gh.runGh }),
    /拒绝修改已发布/,
  );
  assert.equal(gh.calls.length, 1);
});

test("上传后回读缺少、意外或重复资产时 fail closed", async (context) => {
  const mutations = [
    (release) => { release.assets.pop(); },
    (release) => { release.assets.push({ id: 999, name: "unexpected.bin" }); },
    (release) => { release.assets.push({ id: 999, name: release.assets[0].name }); },
  ];
  for (const mutate of mutations) {
    await context.test(mutate.toString().slice(0, 40), async () => {
      const gh = createGhAdapter(undefined, { afterUpload: mutate });
      await assert.rejects(
        reconcileDraftRelease(options(), { runGh: gh.runGh }),
        /上传后 Draft 资产集合不精确/,
      );
    });
  }
});

test("协调接口拒绝错误仓库、tag/version 和非精确上传集合", async () => {
  const gh = createGhAdapter();
  await assert.rejects(
    reconcileDraftRelease({ ...options(), repository: "private/source" }, { runGh: gh.runGh }),
    /只允许协调公开仓库/,
  );
  await assert.rejects(
    reconcileDraftRelease({ ...options(), version: "2.3.5" }, { runGh: gh.runGh }),
    /不匹配/,
  );
  await assert.rejects(
    reconcileDraftRelease({ ...options(), artifacts: artifactPaths().slice(1) }, { runGh: gh.runGh }),
    /缺少/,
  );
  assert.equal(gh.calls.length, 0);
});

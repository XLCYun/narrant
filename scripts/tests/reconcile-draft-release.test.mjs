import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";
import { setImmediate as waitImmediate } from "node:timers/promises";

import { releaseAssetNames } from "../release-assets.mjs";
import {
  reconcileDraftRelease,
  runGhCommand,
} from "../reconcile-draft-release.mjs";

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
  let idReadCount = 0;
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
    if (
      arguments_[0] === "api" &&
      arguments_.length === 2 &&
      /\/releases\/\d+$/.test(arguments_[1])
    ) {
      idReadCount += 1;
      if (hooks.publishOnIdRead === idReadCount) {
        release.draft = false;
      }
      hooks.onIdRead?.(release, idReadCount);
      return { exitCode: 0, stdout: JSON.stringify(release), stderr: "" };
    }
    if (arguments_[0] === "api" && arguments_.includes("DELETE")) {
      const id = Number(arguments_.at(-1).split("/").at(-1));
      release.assets = release.assets.filter((asset) => asset.id !== id);
      if (hooks.publishAfterDelete) release.draft = false;
      return { exitCode: 0, stdout: "", stderr: "" };
    }
    if (arguments_[0] === "release" && arguments_[1] === "create") {
      release = { id: 42, draft: true, tag_name: arguments_[2], assets: [] };
      if (hooks.publishAfterCreate) release.draft = false;
      return { exitCode: 0, stdout: "", stderr: "" };
    }
    if (arguments_[0] === "api" && arguments_.includes("PATCH")) {
      if (hooks.publishAfterEdit) release.draft = false;
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
  return { runGh, calls, release: () => release, idReadCount: () => idReadCount };
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
  assert.equal(gh.calls.some((call) => call.includes("PATCH")), true);
  assert.equal(gh.calls.some((call) => call.includes("--draft") && call[1] !== "create"), false);
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

test("create 后先回读 Draft 身份，published 时不会进入 upload", async () => {
  const gh = createGhAdapter(undefined, { publishAfterCreate: true });
  await assert.rejects(
    reconcileDraftRelease(options(), { runGh: gh.runGh }),
    /创建后拒绝修改已发布/,
  );
  assert.equal(
    gh.calls.some((call) => call[0] === "release" && call[1] === "upload"),
    false,
  );
});

test("release-ID guard 拒绝 ID 或 tag 身份漂移", async (context) => {
  for (const mutation of [
    (release) => { release.id = 99; },
    (release) => { release.tag_name = "v9.9.9"; },
  ]) {
    await context.test(mutation.toString(), async () => {
      const gh = createGhAdapter({
        id: 42,
        draft: true,
        tag_name: "v2.3.4",
        assets: [],
      }, {
        onIdRead: (release, count) => { if (count === 1) mutation(release); },
      });
      await assert.rejects(
        reconcileDraftRelease(options(), { runGh: gh.runGh }),
        /Release ID\/tag 已变化/,
      );
      assert.equal(gh.calls.some((call) => call.includes("PATCH")), false);
    });
  }
});

test("delete、edit、upload 各 mutation 边界在 published 切换后均先停止", async (context) => {
  const cases = [
    {
      name: "delete 前",
      release: { id: 42, draft: true, tag_name: "v2.3.4", assets: [{ id: 7, name: "old.bin" }] },
      publishOnIdRead: 1,
      forbidden: (call) => call.includes("DELETE"),
    },
    {
      name: "edit 前",
      release: { id: 42, draft: true, tag_name: "v2.3.4", assets: [{ id: 7, name: "old.bin" }] },
      publishOnIdRead: 3,
      forbidden: (call) => call.includes("PATCH"),
    },
    {
      name: "upload 前",
      release: { id: 42, draft: true, tag_name: "v2.3.4", assets: [] },
      publishOnIdRead: 3,
      forbidden: (call) => call[0] === "release" && call[1] === "upload",
    },
  ];
  for (const fixture of cases) {
    await context.test(fixture.name, async () => {
      const gh = createGhAdapter(fixture.release, { publishOnIdRead: fixture.publishOnIdRead });
      await assert.rejects(
        reconcileDraftRelease(options(), { runGh: gh.runGh }),
        /拒绝修改已发布/,
      );
      assert.equal(gh.calls.some(fixture.forbidden), false);
    });
  }
});

test("每个 mutation 后立即回读，published 切换会阻止下一类 mutation", async () => {
  const afterDelete = createGhAdapter({
    id: 42,
    draft: true,
    tag_name: "v2.3.4",
    assets: [{ id: 7, name: "old.bin" }],
  }, { publishAfterDelete: true });
  await assert.rejects(
    reconcileDraftRelease(options(), { runGh: afterDelete.runGh }),
    /删除旧资产后拒绝修改已发布/,
  );
  assert.equal(afterDelete.calls.some((call) => call.includes("PATCH")), false);

  const afterEdit = createGhAdapter({
    id: 42,
    draft: true,
    tag_name: "v2.3.4",
    assets: [],
  }, { publishAfterEdit: true });
  await assert.rejects(
    reconcileDraftRelease(options(), { runGh: afterEdit.runGh }),
    /更新元数据后拒绝修改已发布/,
  );
  assert.equal(
    afterEdit.calls.some((call) => call[0] === "release" && call[1] === "upload"),
    false,
  );

  const afterUpload = createGhAdapter(undefined, {
    afterUpload: (release) => { release.draft = false; },
  });
  await assert.rejects(
    reconcileDraftRelease(options(), { runGh: afterUpload.runGh }),
    /上传后拒绝修改已发布/,
  );
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

test("gh runner 等到 close 才返回 exit 后追加的完整 stdout/stderr", async () => {
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  const resultPromise = runGhCommand(["api", "repos/cyunlab/narrant/releases/42"], {
    spawnProcess: () => child,
  });
  let settled = false;
  resultPromise.then(() => { settled = true; });

  child.stdout.emit("data", "{\"id\":42,");
  child.emit("exit", 0);
  child.stdout.emit("data", "\"draft\":true}");
  child.stderr.emit("data", "完整诊断");
  await waitImmediate();
  assert.equal(settled, false);

  child.emit("close", 0);
  const result = await resultPromise;
  assert.deepEqual(result, {
    exitCode: 0,
    stdout: "{\"id\":42,\"draft\":true}",
    stderr: "完整诊断",
  });
});

test("gh runner 的 error 只记录一次并仍等待 close/pipe drain", async () => {
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  const resultPromise = runGhCommand(["api", "broken"], { spawnProcess: () => child });
  child.emit("error", new Error("spawn failed"));
  child.stderr.emit("data", "pipe detail");
  child.emit("close", null);
  child.emit("close", 0);

  assert.deepEqual(await resultPromise, {
    exitCode: -1,
    stdout: "",
    stderr: "pipe detail\nspawn failed",
  });
});

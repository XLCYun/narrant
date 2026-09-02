import assert from "node:assert/strict";
import { Blob, Buffer } from "node:buffer";
import { EventEmitter } from "node:events";
import { ReadableStream } from "node:stream/web";
import test from "node:test";
import { setImmediate as waitImmediate } from "node:timers/promises";

import { releaseAssetNames } from "../release-assets.mjs";
import {
  reconcileDraftRelease,
  runGhCommand,
  uploadReleaseAsset,
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

/** 判断一条 fake adapter 调用是否命中固定 release-ID 上传边界。 */
function isAssetUploadCall(call) {
  return call[0] === "uploadAsset";
}

/** 创建有状态的内存 adapter，覆盖查询、创建、编辑、删除和上传语义。 */
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
    throw new Error(`测试 gh adapter 收到未知命令：${arguments_.join(" ")}`);
  };
  const uploadAsset = async (uploadOptions) => {
    calls.push(["uploadAsset", uploadOptions]);
    const name = uploadOptions.artifact.split(/[\\/]/).at(-1);
    hooks.beforeUpload?.(release, name, uploadOptions);
    if (release.assets.some((asset) => asset.name === name)) {
      throw new Error("duplicate asset (HTTP 422)");
    }
    const asset = {
      id: nextAssetId += 1,
      name,
      state: "uploaded",
      content_type: "application/octet-stream",
      size: 123,
      url: `https://api.github.com/repos/cyunlab/narrant/releases/assets/${nextAssetId}`,
      browser_download_url: `https://github.com/cyunlab/narrant/releases/download/${release.tag_name}/${encodeURIComponent(name)}`,
    };
    release.assets.push(asset);
    hooks.afterUpload?.(release, asset, uploadOptions);
    return asset;
  };
  return { runGh, uploadAsset, calls, release: () => release, idReadCount: () => idReadCount };
}

/** 为协调测试注入内存 gh/upload adapter 和不会离开进程的测试 token。 */
function adapterDependencies(adapter) {
  return { runGh: adapter.runGh, uploadAsset: adapter.uploadAsset, token: "test-token" };
}

/** 构造符合 GitHub upload asset schema 的成功响应对象。 */
function uploadedAsset(name, tag = "v2.3.4") {
  return {
    id: 101,
    name,
    state: "uploaded",
    content_type: "application/octet-stream",
    size: 6,
    url: "https://api.github.com/repos/cyunlab/narrant/releases/assets/101",
    browser_download_url: `https://github.com/cyunlab/narrant/releases/download/${tag}/${encodeURIComponent(name)}`,
  };
}

/** 构造可分块、可声明错误边界的 fetch 响应，全程不访问网络。 */
function uploadResponse(body, settings = {}) {
  const bytes = Buffer.isBuffer(body)
    ? body
    : Buffer.from(typeof body === "string" ? body : JSON.stringify(body));
  const chunks = settings.chunks ?? [bytes];
  const contentLength = settings.contentLength === undefined ? bytes.length : settings.contentLength;
  return {
    status: settings.status ?? 201,
    redirected: settings.redirected ?? false,
    headers: {
      get: (name) => {
        if (name.toLowerCase() === "content-type") return settings.contentType ?? "application/json; charset=utf-8";
        if (name.toLowerCase() === "content-length") return contentLength === null ? null : String(contentLength);
        return null;
      },
    },
    body: new ReadableStream({
      start(controller) {
        for (const chunk of chunks) controller.enqueue(chunk);
        controller.close();
      },
    }),
  };
}

/** 构造可观测 cancel/release 次数且能注入敏感 lifecycle cause 的 fake response body。 */
function trackedResponseBody(chunks, lifecycle, failures = {}) {
  return {
    async cancel() {
      lifecycle.bodyCancel += 1;
      if (failures.bodyCancel) throw new Error(failures.bodyCancel);
    },
    getReader() {
      lifecycle.getReader += 1;
      if (failures.getReader) throw new Error(failures.getReader);
      let cursor = 0;
      return {
        async read() {
          lifecycle.read += 1;
          if (failures.read) throw new Error(failures.read);
          if (cursor >= chunks.length) return { done: true, value: undefined };
          const value = chunks[cursor];
          cursor += 1;
          return { done: false, value };
        },
        async cancel() {
          lifecycle.readerCancel += 1;
          if (failures.readerCancel) throw new Error(failures.readerCancel);
        },
        releaseLock() {
          lifecycle.releaseLock += 1;
          if (failures.releaseLock) throw new Error(failures.releaseLock);
        },
      };
    },
  };
}

/** 创建 lifecycle 计数器，避免各测试遗漏某一条清理路径。 */
function lifecycleCounts() {
  return { bodyCancel: 0, getReader: 0, read: 0, readerCancel: 0, releaseLock: 0 };
}

test("不存在时创建 Draft、上传八文件并通过 API 回读精确集合", async () => {
  const gh = createGhAdapter();
  const result = await reconcileDraftRelease(options(), adapterDependencies(gh));

  assert.deepEqual(result, { outcome: "created", releaseId: 42 });
  assert.deepEqual(
    gh.release().assets.map((asset) => asset.name).sort(),
    artifactPaths().map((path) => path.split("/").at(-1)).sort(),
  );
  assert.equal(gh.calls.some((call) => call[1] === "create" && call.includes("--draft")), true);
  const uploads = gh.calls.filter(isAssetUploadCall);
  assert.equal(uploads.length, 8);
  assert.equal(uploads.every((call) => call[1].releaseId === 42), true);
  assert.equal(uploads.every((call) => call[1].tag === "v2.3.4"), true);
  assert.equal(uploads.every((call) => call[1].token === "test-token"), true);
});

test("已有 Draft 被完整替换，失败后的同 tag 重试保持幂等", async () => {
  const gh = createGhAdapter({
    id: 42,
    draft: true,
    tag_name: "v2.3.4",
    assets: [{ id: 7, name: "partial.bin" }, { id: 8, name: "latest.json" }],
  });
  const first = await reconcileDraftRelease(options(), adapterDependencies(gh));
  const second = await reconcileDraftRelease(options(), adapterDependencies(gh));

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
    reconcileDraftRelease(options(), adapterDependencies(gh)),
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
    reconcileDraftRelease(options(), adapterDependencies(gh)),
    /拒绝修改已发布/,
  );
  assert.equal(gh.calls.length, 1);
});

test("create 后先回读 Draft 身份，published 时不会进入 upload", async () => {
  const gh = createGhAdapter(undefined, { publishAfterCreate: true });
  await assert.rejects(
    reconcileDraftRelease(options(), adapterDependencies(gh)),
    /创建后拒绝修改已发布/,
  );
  assert.equal(
    gh.calls.some(isAssetUploadCall),
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
        reconcileDraftRelease(options(), adapterDependencies(gh)),
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
      forbidden: isAssetUploadCall,
    },
  ];
  for (const fixture of cases) {
    await context.test(fixture.name, async () => {
      const gh = createGhAdapter(fixture.release, { publishOnIdRead: fixture.publishOnIdRead });
      await assert.rejects(
        reconcileDraftRelease(options(), adapterDependencies(gh)),
        /拒绝修改已发布/,
      );
      assert.equal(gh.calls.some(fixture.forbidden), false);
    });
  }
});

test("upload 边界的 tag identity drift 会停止，guard 后 retag 仍只命中固定 release ID", async () => {
  const beforeUpload = createGhAdapter({
    id: 42,
    draft: true,
    tag_name: "v2.3.4",
    assets: [],
  }, {
    onIdRead: (release, count) => { if (count === 3) release.tag_name = "v9.9.9"; },
  });
  await assert.rejects(
    reconcileDraftRelease(options(), adapterDependencies(beforeUpload)),
    /Release ID\/tag 已变化/,
  );
  assert.equal(beforeUpload.calls.some(isAssetUploadCall), false);

  const afterGuard = createGhAdapter(undefined, {
    beforeUpload: (release) => { release.tag_name = "v9.9.9"; },
  });
  await assert.rejects(
    reconcileDraftRelease(options(), adapterDependencies(afterGuard)),
    /Release ID\/tag 已变化/,
  );
  const uploadCalls = afterGuard.calls.filter(isAssetUploadCall);
  assert.equal(uploadCalls.length, 1);
  assert.equal(uploadCalls[0][1].releaseId, 42);
  assert.equal(uploadCalls[0][1].tag, "v2.3.4");
});

test("每个 mutation 后立即回读，published 切换会阻止下一类 mutation", async () => {
  const afterDelete = createGhAdapter({
    id: 42,
    draft: true,
    tag_name: "v2.3.4",
    assets: [{ id: 7, name: "old.bin" }],
  }, { publishAfterDelete: true });
  await assert.rejects(
    reconcileDraftRelease(options(), adapterDependencies(afterDelete)),
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
    reconcileDraftRelease(options(), adapterDependencies(afterEdit)),
    /更新元数据后拒绝修改已发布/,
  );
  assert.equal(
    afterEdit.calls.some(isAssetUploadCall),
    false,
  );

  const afterUpload = createGhAdapter(undefined, {
    afterUpload: (release) => { release.draft = false; },
  });
  await assert.rejects(
    reconcileDraftRelease(options(), adapterDependencies(afterUpload)),
    /上传 .* 后拒绝修改已发布/,
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
        reconcileDraftRelease(options(), adapterDependencies(gh)),
        /上传后 Draft 资产集合不精确/,
      );
    });
  }
});

test("release-ID endpoint 遇到重复名直接失败且不使用 clobber", async () => {
  const gh = createGhAdapter(undefined, {
    beforeUpload: (release, name) => {
      release.assets.push({ id: 999, name });
    },
  });
  await assert.rejects(
    reconcileDraftRelease(options(), adapterDependencies(gh)),
    /duplicate asset.*422/,
  );
  assert.equal(gh.calls.some((call) => call.includes("--clobber")), false);
});

test("协调接口拒绝错误仓库、tag/version 和非精确上传集合", async () => {
  const gh = createGhAdapter();
  await assert.rejects(
    reconcileDraftRelease({ ...options(), repository: "private/source" }, adapterDependencies(gh)),
    /只允许协调公开仓库/,
  );
  await assert.rejects(
    reconcileDraftRelease({ ...options(), version: "2.3.5" }, adapterDependencies(gh)),
    /不匹配/,
  );
  await assert.rejects(
    reconcileDraftRelease({ ...options(), artifacts: artifactPaths().slice(1) }, adapterDependencies(gh)),
    /缺少/,
  );
  assert.equal(gh.calls.length, 0);
});

test("生产上传 adapter 固定 release-ID endpoint、仅以 header 携 token 并流式发送 Blob", async () => {
  const token = "secret-token-never-log";
  const artifact = "/tmp/Narrant package.dmg";
  const requestBody = new Blob([Buffer.from("binary")], { type: "application/octet-stream" });
  let observed;
  const result = await uploadReleaseAsset({
    repository: "cyunlab/narrant",
    tag: "v2.3.4",
    releaseId: 42,
    artifact,
    token,
  }, {
    openBlob: async (path, settings) => {
      assert.equal(path, artifact);
      assert.deepEqual(settings, { type: "application/octet-stream" });
      return requestBody;
    },
    fetchImpl: async (url, request) => {
      observed = { url, request };
      return uploadResponse(uploadedAsset("Narrant package.dmg"));
    },
  });

  assert.equal(observed.url, "https://uploads.github.com/repos/cyunlab/narrant/releases/42/assets?name=Narrant%20package.dmg");
  assert.equal(observed.url.includes(token), false);
  assert.equal(observed.request.method, "POST");
  assert.equal(observed.request.redirect, "error");
  assert.equal(observed.request.headers.Authorization, `Bearer ${token}`);
  assert.equal(observed.request.headers["Content-Type"], "application/octet-stream");
  assert.equal(observed.request.body, requestBody);
  assert.equal(Buffer.from(await observed.request.body.arrayBuffer()).toString(), "binary");
  assert.equal(result.name, "Narrant package.dmg");
});

test("生产上传 adapter 在 token 缺失时不会读取文件或发起请求", async (context) => {
  for (const token of [undefined, "", " token", "token "]) {
    await context.test(String(token), async () => {
      let touched = false;
      await assert.rejects(
        uploadReleaseAsset({
          repository: "cyunlab/narrant",
          tag: "v2.3.4",
          releaseId: 42,
          artifact: "/tmp/file.bin",
          token,
        }, {
          openBlob: async () => { touched = true; },
          fetchImpl: async () => { touched = true; },
        }),
        /需要有效 GH_TOKEN/,
      );
      assert.equal(touched, false);
    });
  }
});

test("生产上传 adapter 拒绝网络、redirect、非 2xx 与不可信响应 framing", async (context) => {
  const base = {
    repository: "cyunlab/narrant",
    tag: "v2.3.4",
    releaseId: 42,
    artifact: "/tmp/file.bin",
    token: "never-leak-this-token",
  };
  const openBlob = async () => new Blob(["binary"]);
  const fixtures = [
    {
      name: "network",
      fetchImpl: async () => { throw new Error(base.token); },
      pattern: /网络请求失败/,
    },
    {
      name: "redirect",
      fetchImpl: async () => uploadResponse(uploadedAsset("file.bin"), { redirected: true }),
      pattern: /拒绝重定向/,
    },
    {
      name: "non-2xx",
      fetchImpl: async () => uploadResponse({ message: base.token }, { status: 422 }),
      pattern: /HTTP 422/,
    },
    {
      name: "content-type",
      fetchImpl: async () => uploadResponse(uploadedAsset("file.bin"), { contentType: "text/html" }),
      pattern: /错误 Content-Type/,
    },
    {
      name: "invalid content-length",
      fetchImpl: async () => uploadResponse(uploadedAsset("file.bin"), { contentLength: "NaN" }),
      pattern: /无效 Content-Length/,
    },
    {
      name: "declared oversize",
      fetchImpl: async () => uploadResponse(uploadedAsset("file.bin"), { contentLength: 256 * 1024 + 1 }),
      pattern: /超过安全上限/,
    },
    {
      name: "stream oversize",
      fetchImpl: async () => uploadResponse("", {
        contentLength: null,
        chunks: [Buffer.alloc(128 * 1024), Buffer.alloc(128 * 1024 + 1)],
      }),
      pattern: /超过安全上限/,
    },
    {
      name: "invalid utf8",
      fetchImpl: async () => uploadResponse(Buffer.from([0xff])),
      pattern: /无效 UTF-8/,
    },
    {
      name: "invalid json",
      fetchImpl: async () => uploadResponse("not-json"),
      pattern: /无效 JSON/,
    },
  ];
  for (const fixture of fixtures) {
    await context.test(fixture.name, async () => {
      await assert.rejects(
        uploadReleaseAsset(base, { openBlob, fetchImpl: fixture.fetchImpl }),
        (error) => {
          assert.match(error.message, fixture.pattern);
          assert.equal(error.message.includes(base.token), false);
          return true;
        },
      );
    });
  }

  await assert.rejects(
    uploadReleaseAsset(base, {
      openBlob: async () => { throw new Error(base.token); },
      fetchImpl: async () => { throw new Error("不应调用"); },
    }),
    (error) => {
      assert.match(error.message, /无法读取待上传资产/);
      assert.equal(error.message.includes(base.token), false);
      return true;
    },
  );
});

test("response metadata 早退均 best-effort cancel body 且 cleanup cause 不覆盖主错误", async (context) => {
  const token = "token-from-body-cancel";
  const base = {
    repository: "cyunlab/narrant",
    tag: "v2.3.4",
    releaseId: 42,
    artifact: "/tmp/file.bin",
    token: "request-token",
  };
  const fixtures = [
    { name: "redirect", settings: { redirected: true }, pattern: /拒绝重定向/ },
    { name: "non-2xx", settings: { status: 422 }, pattern: /HTTP 422/ },
    { name: "content-type", settings: { contentType: "text/plain" }, pattern: /错误 Content-Type/ },
    { name: "invalid length", settings: { contentLength: "NaN" }, pattern: /无效 Content-Length/ },
    { name: "declared oversize", settings: { contentLength: 256 * 1024 + 1 }, pattern: /超过安全上限/ },
  ];
  for (const fixture of fixtures) {
    await context.test(fixture.name, async () => {
      const lifecycle = lifecycleCounts();
      const response = uploadResponse(uploadedAsset("file.bin"), fixture.settings);
      response.body = trackedResponseBody([], lifecycle, { bodyCancel: token });
      await assert.rejects(
        uploadReleaseAsset(base, {
          openBlob: async () => new Blob(["binary"]),
          fetchImpl: async () => response,
        }),
        (error) => {
          assert.match(error.message, fixture.pattern);
          assert.equal(error.message.includes(token), false);
          return true;
        },
      );
      assert.deepEqual(lifecycle, {
        bodyCancel: 1,
        getReader: 0,
        read: 0,
        readerCancel: 0,
        releaseLock: 0,
      });
    });
  }
});

test("response body/header/getReader cause 固定脱敏并在可用时 cancel body", async (context) => {
  const token = "token-from-response-lifecycle";
  const base = {
    repository: "cyunlab/narrant",
    tag: "v2.3.4",
    releaseId: 42,
    artifact: "/tmp/file.bin",
    token: "request-token",
  };
  await context.test("body getter", async () => {
    const response = uploadResponse(uploadedAsset("file.bin"));
    Object.defineProperty(response, "body", {
      get() { throw new Error(token); },
    });
    await assert.rejects(
      uploadReleaseAsset(base, {
        openBlob: async () => new Blob(["binary"]),
        fetchImpl: async () => response,
      }),
      (error) => {
        assert.match(error.message, /metadata 无法安全读取/);
        assert.equal(error.message.includes(token), false);
        return true;
      },
    );
  });
  await context.test("header getter", async () => {
    const lifecycle = lifecycleCounts();
    const response = uploadResponse(uploadedAsset("file.bin"));
    response.body = trackedResponseBody([], lifecycle);
    response.headers.get = () => { throw new Error(token); };
    await assert.rejects(
      uploadReleaseAsset(base, {
        openBlob: async () => new Blob(["binary"]),
        fetchImpl: async () => response,
      }),
      (error) => {
        assert.match(error.message, /metadata 无法安全读取/);
        assert.equal(error.message.includes(token), false);
        return true;
      },
    );
    assert.equal(lifecycle.bodyCancel, 1);
    assert.equal(lifecycle.getReader, 0);
  });
  await context.test("getReader", async () => {
    const lifecycle = lifecycleCounts();
    const response = uploadResponse(uploadedAsset("file.bin"));
    response.body = trackedResponseBody([], lifecycle, { getReader: token, bodyCancel: token });
    await assert.rejects(
      uploadReleaseAsset(base, {
        openBlob: async () => new Blob(["binary"]),
        fetchImpl: async () => response,
      }),
      (error) => {
        assert.match(error.message, /reader 无法安全创建/);
        assert.equal(error.message.includes(token), false);
        return true;
      },
    );
    assert.equal(lifecycle.getReader, 1);
    assert.equal(lifecycle.bodyCancel, 1);
  });
});

test("reader read、oversize、release 异常均 cancel/release 且敏感 cleanup cause 不外泄", async (context) => {
  const token = "token-from-reader-lifecycle";
  const base = {
    repository: "cyunlab/narrant",
    tag: "v2.3.4",
    releaseId: 42,
    artifact: "/tmp/file.bin",
    token: "request-token",
  };
  const fixtures = [
    {
      name: "read",
      chunks: [],
      failures: { read: token, readerCancel: token, releaseLock: token },
      pattern: /响应读取失败/,
    },
    {
      name: "stream oversize",
      chunks: [Buffer.alloc(256 * 1024 + 1)],
      failures: { readerCancel: token, releaseLock: token },
      pattern: /超过安全上限/,
    },
    {
      name: "release",
      chunks: [Buffer.from(JSON.stringify(uploadedAsset("file.bin")))],
      failures: { readerCancel: token, releaseLock: token },
      pattern: /reader lifecycle 无效/,
    },
  ];
  for (const fixture of fixtures) {
    await context.test(fixture.name, async () => {
      const lifecycle = lifecycleCounts();
      const response = uploadResponse("", { contentLength: null });
      response.body = trackedResponseBody(fixture.chunks, lifecycle, fixture.failures);
      await assert.rejects(
        uploadReleaseAsset(base, {
          openBlob: async () => new Blob(["binary"]),
          fetchImpl: async () => response,
        }),
        (error) => {
          assert.match(error.message, fixture.pattern);
          assert.equal(error.message.includes(token), false);
          return true;
        },
      );
      assert.equal(lifecycle.readerCancel, 1);
      assert.equal(lifecycle.releaseLock, 1);
      assert.equal(lifecycle.bodyCancel, 0);
    });
  }
});

test("本地 Blob.size 必须逐字节等于 GitHub asset.size，size getter cause 不外泄", async () => {
  const token = "token-from-blob-size";
  const base = {
    repository: "cyunlab/narrant",
    tag: "v2.3.4",
    releaseId: 42,
    artifact: "/tmp/file.bin",
    token: "request-token",
  };
  const oneByteAsset = uploadedAsset("file.bin");
  oneByteAsset.size = 1;
  assert.equal((await uploadReleaseAsset(base, {
    openBlob: async () => new Blob(["x"]),
    fetchImpl: async () => uploadResponse(oneByteAsset),
  })).size, 1);

  const wrongSize = uploadedAsset("file.bin");
  wrongSize.size = 999;
  await assert.rejects(
    uploadReleaseAsset(base, {
      openBlob: async () => new Blob(["x"]),
      fetchImpl: async () => uploadResponse(wrongSize),
    }),
    /无效 asset identity/,
  );

  let fetched = false;
  await assert.rejects(
    uploadReleaseAsset(base, {
      openBlob: async () => ({ get size() { throw new Error(token); } }),
      fetchImpl: async () => { fetched = true; },
    }),
    (error) => {
      assert.match(error.message, /无法读取待上传资产/);
      assert.equal(error.message.includes(token), false);
      return true;
    },
  );
  assert.equal(fetched, false);
});

test("生产上传 adapter 对每个 asset response identity 与 exact API URL fail closed", async (context) => {
  const base = {
    repository: "cyunlab/narrant",
    tag: "v2.3.4",
    releaseId: 42,
    artifact: "/tmp/file.bin",
    token: "test-token",
  };
  const mutations = [
    (asset) => { delete asset.id; },
    (asset) => { asset.id = 1.5; },
    (asset) => { asset.name = "wrong.bin"; },
    (asset) => { asset.state = "starter"; },
    (asset) => { asset.content_type = "text/plain"; },
    (asset) => { asset.size = 0; },
    (asset) => { asset.size = 999; },
    (asset) => { asset.url = "https://api.github.com:443/repos/cyunlab/narrant/releases/assets/101"; },
    (asset) => { asset.url += "?private=1"; },
    (asset) => { asset.url += "#fragment"; },
    (asset) => { asset.url = "https://user@api.github.com/repos/cyunlab/narrant/releases/assets/101"; },
    (asset) => { asset.url = "https://api.github.com/repos/other/repo/releases/assets/101"; },
    (asset) => { asset.browser_download_url = "https://github.com/other/repo/file"; },
  ];
  for (const mutation of mutations) {
    await context.test(mutation.toString(), async () => {
      const asset = uploadedAsset("file.bin");
      mutation(asset);
      await assert.rejects(
        uploadReleaseAsset(base, {
          openBlob: async () => new Blob(["binary"]),
          fetchImpl: async () => uploadResponse(asset),
        }),
        /无效 asset identity|错误仓库或下载 URL/,
      );
    });
  }
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

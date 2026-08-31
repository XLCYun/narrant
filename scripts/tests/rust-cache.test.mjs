import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import {
  assertSafeCacheDirectory,
  resolveCacheDirectory,
} from "../rust-cache.mjs";

test("macOS 使用官方默认的用户缓存目录", () => {
  const directory = resolveCacheDirectory({
    env: { HOME: "/Users/developer" },
    platform: "darwin",
  });

  assert.equal(directory, path.join("/Users/developer", "Library", "Caches", "Mozilla.sccache"));
});

test("显式 SCCACHE_DIR 覆盖平台默认目录", () => {
  const directory = resolveCacheDirectory({
    env: { HOME: "/Users/developer", SCCACHE_DIR: "/tmp/team-sccache" },
    platform: "darwin",
  });

  assert.equal(directory, "/tmp/team-sccache");
});

test("安全校验接受具体的 sccache 目录", () => {
  const directory = assertSafeCacheDirectory("/tmp/narrant/sccache", {
    env: { HOME: "/Users/developer" },
    repositoryRoot: "/workspace/narrant",
  });

  assert.equal(directory, "/tmp/narrant/sccache");
});

test("安全校验拒绝用户目录和无 sccache 标识的目录", () => {
  assert.throws(
    () => assertSafeCacheDirectory("/Users/developer", {
      env: { HOME: "/Users/developer" },
      repositoryRoot: "/workspace/narrant",
    }),
    /过于宽泛/,
  );
  assert.throws(
    () => assertSafeCacheDirectory("/tmp/build-cache", {
      env: { HOME: "/Users/developer" },
      repositoryRoot: "/workspace/narrant",
    }),
    /不像 sccache/,
  );
});

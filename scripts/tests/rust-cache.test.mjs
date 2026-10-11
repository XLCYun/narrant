import assert from "node:assert/strict";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
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

// 真实原生子进程必须逐字收到带 TOML 引号、空格和反斜杠的参数，不再经过 cmd.exe。
test("Rust 缓存入口保留原生命令的结构化参数", (t) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "narrant argv 空格-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const fixture = path.join(directory, "runner fixture.mjs");
  fs.writeFileSync(fixture, "process.stdout.write(JSON.stringify(process.argv.slice(2)));\n");
  const args = ["--config", `target.'cfg(all())'.runner=${JSON.stringify(["C:\\Program Files\\node.exe", "D:\\repo 空格\\runner.mjs"])}`];
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("../rust-cache.mjs", import.meta.url)), "run", process.execPath, fixture, ...args], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), args);
});

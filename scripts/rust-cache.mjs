#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import console from "node:console";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const DEFAULT_CACHE_SIZE = "2G";
const SCRIPT_PATH = fileURLToPath(import.meta.url);
const REPOSITORY_ROOT = path.resolve(path.dirname(SCRIPT_PATH), "..");

/** 解析当前平台使用的 sccache 本地缓存目录。 */
export function resolveCacheDirectory({ env = process.env, platform = process.platform } = {}) {
  if (env.SCCACHE_DIR) {
    return path.resolve(env.SCCACHE_DIR);
  }

  if (platform === "win32") {
    const localAppData = env.LOCALAPPDATA;
    if (!localAppData) {
      throw new Error("Windows 环境缺少 LOCALAPPDATA，无法确定 sccache 缓存目录");
    }
    return path.join(localAppData, "Mozilla", "sccache");
  }

  const userHome = env.HOME || homedir();
  if (!userHome) {
    throw new Error("无法确定用户目录，不能配置 sccache 缓存");
  }

  if (platform === "darwin") {
    return path.join(userHome, "Library", "Caches", "Mozilla.sccache");
  }

  return path.join(env.XDG_CACHE_HOME || path.join(userHome, ".cache"), "sccache");
}

/** 验证待清理目录确实是一个具体的 sccache 缓存目录。 */
export function assertSafeCacheDirectory(cacheDirectory, {
  env = process.env,
  repositoryRoot = REPOSITORY_ROOT,
} = {}) {
  const resolved = path.resolve(cacheDirectory);
  const filesystemRoot = path.parse(resolved).root;
  const userHome = path.resolve(env.HOME || homedir());
  const normalized = resolved.toLowerCase();
  const components = normalized.split(path.sep).filter(Boolean);
  const namesSccache = components.some(
    (component) => component === "sccache" || component === "mozilla.sccache",
  );

  if (!path.isAbsolute(cacheDirectory)) {
    throw new Error("sccache 缓存目录必须是绝对路径");
  }
  if (resolved === filesystemRoot || resolved === userHome || resolved === path.resolve(repositoryRoot)) {
    throw new Error(`拒绝清理过于宽泛的路径：${resolved}`);
  }
  if (!namesSccache) {
    throw new Error(`拒绝清理名称不像 sccache 缓存的路径：${resolved}`);
  }

  return resolved;
}

/** 检查 sccache 是否已安装且可以执行。 */
function hasSccache() {
  const result = spawnSync("sccache", ["--version"], {
    encoding: "utf8",
    shell: process.platform === "win32",
    stdio: "ignore",
  });
  return result.status === 0;
}

/** 返回包含统一缓存目录、容量限制和 rustc wrapper 的构建环境。 */
export function createBuildEnvironment() {
  const env = {
    ...process.env,
    SCCACHE_CACHE_SIZE: process.env.SCCACHE_CACHE_SIZE || DEFAULT_CACHE_SIZE,
    SCCACHE_DIR: resolveCacheDirectory(),
  };

  if (!env.RUSTC_WRAPPER && hasSccache()) {
    env.RUSTC_WRAPPER = "sccache";
  }

  return env;
}

/** 输出当前平台的 sccache 安装提示。 */
function printInstallHint() {
  if (process.platform === "darwin") {
    console.error("安装命令：brew install sccache");
    return;
  }
  if (process.platform === "win32") {
    console.error("安装命令：winget install Mozilla.sccache");
    return;
  }
  console.error("请通过系统包管理器安装 sccache，或执行：cargo install sccache --locked");
}

/** 在统一 Rust 缓存环境中运行指定命令。 */
function runCommand(command, args) {
  const env = createBuildEnvironment();
  if (!env.RUSTC_WRAPPER) {
    console.error("warning: 未找到 sccache，本次 Rust 构建将继续但不能复用跨 worktree 缓存");
    printInstallHint();
  } else if (env.RUSTC_WRAPPER !== "sccache") {
    console.error(`warning: 保留已有 RUSTC_WRAPPER=${env.RUSTC_WRAPPER}，未强制改为 sccache`);
  }

  const child = spawn(command, args, {
    env,
    shell: process.platform === "win32",
    stdio: "inherit",
  });

  child.on("error", (error) => {
    console.error(`无法启动命令 ${command}：${error.message}`);
    process.exitCode = 1;
  });
  child.on("close", (code, signal) => {
    if (signal) {
      console.error(`命令 ${command} 被信号 ${signal} 终止`);
      process.exitCode = 1;
      return;
    }
    process.exitCode = code ?? 1;
  });
}

/** 显示 sccache 位置、容量约束和命中统计。 */
function showStatus() {
  if (!hasSccache()) {
    console.error("sccache 尚未安装，Rust 构建会自动降级为普通 Cargo 编译。");
    printInstallHint();
    process.exitCode = 1;
    return;
  }

  const env = createBuildEnvironment();
  console.log(`缓存目录：${env.SCCACHE_DIR}`);
  console.log(`容量上限：${env.SCCACHE_CACHE_SIZE}`);
  console.log("Cargo incremental：遵循 dev/test profile 默认值，本脚本不关闭它");
  const result = spawnSync("sccache", ["--show-stats"], {
    env,
    shell: process.platform === "win32",
    stdio: "inherit",
  });
  process.exitCode = result.status ?? 1;
}

/** 停止本地服务并删除 sccache 内容，但不触碰任何 worktree 的 target。 */
function cleanCache() {
  if (!hasSccache()) {
    console.error("sccache 尚未安装，没有可清理的 sccache 缓存。");
    process.exitCode = 1;
    return;
  }

  const env = createBuildEnvironment();
  const cacheDirectory = assertSafeCacheDirectory(env.SCCACHE_DIR);
  spawnSync("sccache", ["--stop-server"], {
    env,
    shell: process.platform === "win32",
    stdio: "ignore",
  });

  if (existsSync(cacheDirectory)) {
    rmSync(cacheDirectory, { force: true, recursive: true });
  }
  mkdirSync(cacheDirectory, { recursive: true });
  console.log(`已清理 sccache 缓存：${cacheDirectory}`);
  console.log("各 worktree 的 target 和 Cargo incremental 数据均未删除。");
}

/** 解析命令行并分派缓存操作。 */
function main() {
  const [action, ...args] = process.argv.slice(2);
  if (action === "run") {
    const [command, ...commandArgs] = args;
    if (!command) {
      throw new Error("用法：rust-cache.mjs run <command> [args...]");
    }
    runCommand(command, commandArgs);
    return;
  }
  if (action === "status") {
    showStatus();
    return;
  }
  if (action === "clean") {
    cleanCache();
    return;
  }
  throw new Error("用法：rust-cache.mjs <run|status|clean>");
}

if (process.argv[1] && path.resolve(process.argv[1]) === SCRIPT_PATH) {
  try {
    main();
  } catch (error) {
    console.error(`错误：${error.message}`);
    process.exitCode = 1;
  }
}

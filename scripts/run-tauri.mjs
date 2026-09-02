#!/usr/bin/env node

import { spawn } from "node:child_process";
import { Buffer } from "node:buffer";
import console from "node:console";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { createBuildEnvironment } from "./rust-cache.mjs";

const SCRIPT_PATH = fileURLToPath(import.meta.url);

/** 验证正式构建使用的是 canonical、两行且 base64 编码的 minisign 公钥。 */
export function normalizeUpdaterPublicKey(value) {
  const normalized = value.trim();
  let decoded;
  try {
    decoded = Buffer.from(normalized, "base64").toString("utf8");
  } catch {
    throw new Error("NARRANT_UPDATER_PUBLIC_KEY 必须是 base64 编码的两行 minisign 公钥");
  }
  if (
    !/^[A-Za-z0-9+/]+={0,2}$/.test(normalized)
    || Buffer.from(decoded, "utf8").toString("base64") !== normalized
    || !/^untrusted comment: minisign public key: [0-9A-F]+\nRW[A-Za-z0-9+/=]+$/.test(decoded)
  ) {
    throw new Error("NARRANT_UPDATER_PUBLIC_KEY 必须是 base64 编码的两行 minisign 公钥");
  }
  return normalized;
}

/** 只在正式 build 时把编译期 updater 信任根同时交给 Tauri bundler。 */
export function buildTauriArguments(command, args, environment) {
  const cliArguments = [command];
  if (command === "build" && environment.NARRANT_UPDATER_PUBLIC_KEY) {
    const pubkey = normalizeUpdaterPublicKey(environment.NARRANT_UPDATER_PUBLIC_KEY);
    cliArguments.push("--config", JSON.stringify({ plugins: { updater: { pubkey } } }));
  }
  const normalizedArguments = args[0] === "--" ? args.slice(1) : args;
  return [...cliArguments, ...normalizedArguments];
}

/** 通过当前 pnpm 入口构造无 shell 的 Tauri 调用，确保 Windows 不会剥掉 JSON 引号。 */
export function buildTauriInvocation(command, args, environment, nodeExecutable = process.execPath) {
  const pnpmEntry = environment.npm_execpath;
  if (!pnpmEntry) throw new Error("Tauri 启动器必须从 pnpm script 运行");
  return {
    args: [pnpmEntry, "exec", "tauri", ...buildTauriArguments(command, args, environment)],
    executable: nodeExecutable,
  };
}

/** 经仓库统一 sccache 环境启动 Tauri，并原样转发终端与退出状态。 */
function run() {
  const [command, ...args] = process.argv.slice(2);
  if (!command) throw new Error("缺少 Tauri 子命令");
  const invocation = buildTauriInvocation(command, args, process.env);
  const child = spawn(invocation.executable, invocation.args, {
    env: createBuildEnvironment(),
    shell: false,
    stdio: "inherit",
  });
  child.once("error", (error) => {
    console.error(`无法启动 Tauri：${error.message}`);
    process.exitCode = 1;
  });
  child.once("exit", (code, signal) => {
    if (signal) {
      console.error(`Tauri 被信号 ${signal} 终止`);
      process.exitCode = 1;
      return;
    }
    process.exitCode = code ?? 1;
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === SCRIPT_PATH) {
  try {
    run();
  } catch (error) {
    console.error(`错误：${error.message}`);
    process.exitCode = 1;
  }
}

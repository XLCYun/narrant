import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import process from "node:process";

/** 读取本地开发连接，但绝不把 endpoint 或凭据写入输出。 */
async function loadLiveConnection() {
  const defaultSettings = join(
    homedir(),
    "Library",
    "Application Support",
    "com.cyunlab.narrant",
    "settings.json",
  );
  const settingsPath = resolve(process.env.NARRANT_LIVE_SETTINGS_PATH || defaultSettings);
  const credentialPath = join(dirname(settingsPath), "custom-provider-credential.dev.json");
  const settings = JSON.parse(await readFile(settingsPath, "utf8"));
  let credential = {};
  try {
    credential = JSON.parse(await readFile(credentialPath, "utf8"));
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
  const custom = settings?.babysit?.custom;
  const apiKey = credential?.apiKey ?? "";
  if (custom?.protocol !== "openai_chat_completions") {
    throw new Error("live Custom Provider E2E 当前只接受显式 OpenAI Chat Completions 配置");
  }
  if (typeof custom.baseUrl !== "string" || !custom.baseUrl.trim()) {
    throw new Error("本地 Custom Provider 缺少 Base URL");
  }
  if (typeof custom.model !== "string" || !custom.model.trim()) {
    throw new Error("本地 Custom Provider 缺少模型标识");
  }
  if (typeof apiKey !== "string") {
    throw new Error("本地 Custom Provider 开发凭据格式无效");
  }
  return {
    baseUrl: custom.baseUrl,
    model: custom.model,
    effort: custom.effort || "provider_default",
    apiKey,
  };
}

const connection = await loadLiveConnection();
const child = spawn(
  "pnpm",
  ["run", "test:e2e", "--spec", "./e2e/live-custom-provider.smoke.ts"],
  {
    cwd: process.cwd(),
    stdio: "inherit",
    env: {
      ...process.env,
      NARRANT_E2E_LIVE_CUSTOM_PROVIDER: "true",
      NARRANT_E2E_CUSTOM_PROVIDER_BASE_URL: connection.baseUrl,
      NARRANT_E2E_CUSTOM_PROVIDER_MODEL: connection.model,
      NARRANT_E2E_CUSTOM_PROVIDER_EFFORT: connection.effort,
      NARRANT_E2E_CUSTOM_PROVIDER_API_KEY: connection.apiKey,
    },
  },
);

child.on("error", (error) => {
  process.stderr.write(`无法启动 live Custom Provider E2E：${error.message}\n`);
  process.exitCode = 1;
});
child.on("exit", (code, signal) => {
  process.exitCode = signal ? 1 : code ?? 1;
});

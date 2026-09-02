import { createHash } from "node:crypto";
import { Buffer } from "node:buffer";
import { appendFile } from "node:fs/promises";
import process from "node:process";
import { pathToFileURL, URL } from "node:url";
import { TextDecoder } from "node:util";

import { FIXED_STABLE_ENDPOINT } from "./public-draft-assets.mjs";

const MAX_MANIFEST_BYTES = 512 * 1024;
const TARGETS = ["darwin-aarch64", "windows-x86_64"];

/** 匿名读取当前 published Stable，并固定后续 smoke 需要的 tag 与原始字节摘要。 */
export async function readPublicStableIdentity(_options = {}, dependencies = {}) {
  void _options;
  const fetchImpl = dependencies.fetchImpl ?? globalThis.fetch;
  if (typeof fetchImpl !== "function") throw new Error("当前 Node 缺少 fetch");
  let response;
  try {
    response = await fetchImpl(FIXED_STABLE_ENDPOINT, {
      redirect: "follow",
      headers: { Accept: "application/octet-stream" },
    });
  } catch {
    throw new Error("published Stable manifest 匿名下载失败");
  }
  let finalUrl;
  try { finalUrl = new URL(response?.url); } catch { throw new Error("published Stable manifest 最终 URL 无效"); }
  if (
    !response || response.status < 200 || response.status >= 300 ||
    finalUrl.protocol !== "https:" || finalUrl.username || finalUrl.password || finalUrl.port ||
    !["github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"].includes(finalUrl.hostname)
  ) {
    throw new Error("published Stable manifest 响应身份无效");
  }
  const length = response.headers.get("content-length");
  if (length !== null && (!/^\d+$/.test(length) || Number(length) > MAX_MANIFEST_BYTES)) {
    throw new Error("published Stable manifest 超过上限");
  }
  const bytes = Buffer.from(await response.arrayBuffer());
  if (bytes.length <= 0 || bytes.length > MAX_MANIFEST_BYTES) throw new Error("published Stable manifest 大小无效");
  let manifest;
  try { manifest = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)); }
  catch { throw new Error("published Stable manifest 不是有效 JSON"); }
  if (!/^\d+\.\d+\.\d+$/.test(manifest?.version ?? "")) throw new Error("published Stable manifest 版本无效");
  const tag = `v${manifest.version}`;
  if (Object.keys(manifest.platforms ?? {}).sort().join(",") !== [...TARGETS].sort().join(",")) {
    throw new Error("published Stable manifest 目标集合无效");
  }
  const prefix = `https://github.com/cyunlab/narrant/releases/download/${tag}/`;
  for (const target of TARGETS) {
    const entry = manifest.platforms[target];
    let packageUrl;
    try { packageUrl = new URL(entry?.url); } catch { throw new Error("published Stable manifest package URL 无效"); }
    if (
      !entry?.url.startsWith(prefix) || packageUrl.href !== entry.url || packageUrl.search || packageUrl.hash ||
      packageUrl.username || packageUrl.password || packageUrl.port || typeof entry.signature !== "string" || !entry.signature
    ) {
      throw new Error("published Stable manifest package identity 无效");
    }
  }
  return Object.freeze({ tag, manifestSha256: createHash("sha256").update(bytes).digest("hex") });
}

/** GitHub Actions CLI 只输出公开 Stable identity，不泄露任何凭据。 */
async function main() {
  const result = await readPublicStableIdentity();
  if (!process.env.GITHUB_OUTPUT) throw new Error("缺少 GITHUB_OUTPUT");
  await appendFile(process.env.GITHUB_OUTPUT, `tag=${result.tag}\nmanifest_sha256=${result.manifestSha256}\n`, "utf8");
  process.stdout.write(`已固定 previous Stable ${result.tag} / ${result.manifestSha256}。\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`错误：${error instanceof Error ? error.message : "Stable identity 读取失败"}\n`);
    process.exitCode = 1;
  });
}

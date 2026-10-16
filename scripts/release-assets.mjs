import {
  createHash,
  createPublicKey,
  verify as verifySignature,
} from "node:crypto";
import { Buffer } from "node:buffer";
import {
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import { basename, join } from "node:path";
import { TextDecoder } from "node:util";
import { gunzipSync } from "node:zlib";

const PUBLIC_REPOSITORY = "XLCYun/narrant";
const STABLE_TAG_PATTERN = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const TARGETS = ["darwin-aarch64", "windows-x86_64"];

/** 按 Unicode code point 稳定排序公开文件名，不依赖 runner locale/ICU。 */
function comparePublicNames(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

/** 校验稳定版 tag 与版本，并返回后续资产逻辑唯一使用的发布身份。 */
export function parseReleaseIdentity(tag, version) {
  const match = STABLE_TAG_PATTERN.exec(String(tag ?? ""));
  if (!match || match.slice(1).join(".") !== version) {
    throw new Error(`发布 tag/version 不匹配：${tag}/${version}`);
  }
  return { tag, version };
}

/** 返回某个版本完整 Draft 必须拥有的公开资产名。 */
export function releaseAssetNames(version) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`无效发布版本：${version}`);
  }
  return {
    installer: {
      darwin: `Narrant_${version}_aarch64.dmg`,
      windows: `Narrant_${version}_x64-setup.exe`,
    },
    updater: {
      darwin: "narrant-darwin-aarch64-updater.app.tar.gz",
      windows: "narrant-windows-x86_64-updater.exe",
    },
    manifest: "latest.json",
    checksums: "SHA256SUMS",
  };
}

/** 构建 tag-specific 的公开下载 URL，不允许调用方注入仓库或 endpoint。 */
function publicDownloadUrl(tag, name) {
  return `https://github.com/${PUBLIC_REPOSITORY}/releases/download/${tag}/${encodeURIComponent(name)}`;
}

/** 严格解码带 padding、无空白且可逐字节 round-trip 的 standard base64。 */
function decodeCanonicalBase64(value, label) {
  const input = String(value ?? "");
  if (!input) {
    throw new Error(`${label} 为空`);
  }
  if (
    input.length % 4 !== 0 ||
    !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(input)
  ) {
    throw new Error(`${label} 不是 canonical base64`);
  }
  const decoded = Buffer.from(input, "base64");
  if (decoded.toString("base64") !== input) {
    throw new Error(`${label} 不是 canonical base64`);
  }
  return decoded;
}

/** 将 Tauri 配置使用的 canonical outer-base64 解码为 minisign 文本。 */
function decodeTauriText(value, label) {
  const decoded = decodeCanonicalBase64(value, label);
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(decoded);
  } catch {
    throw new Error(`${label} 不是有效 UTF-8`);
  }
}

/** 解析 Tauri updater 公钥 packet。 */
function parsePublicKey(encoded) {
  const decoded = decodeTauriText(encoded, "updater 公钥");
  const lines = decoded.split("\n");
  if (
    decoded.includes("\r") ||
    lines.length !== 2 ||
    lines.some((line) => /\s$/.test(line))
  ) {
    throw new Error("updater 公钥 inner text 必须精确两行且 comment 无尾随空白");
  }
  const packet = decodeCanonicalBase64(lines[1] ?? "", "updater 公钥 packet");
  const algorithm = packet.subarray(0, 2).toString("ascii");
  if (packet.length !== 42 || !["Ed", "ED"].includes(algorithm)) {
    throw new Error("updater 公钥结构无效");
  }
  return { id: packet.subarray(2, 10), key: packet.subarray(10) };
}

/** 解析 Tauri `.sig` 中的 minisign packet 与可信注释。 */
function parseUpdaterSignature(encoded) {
  const decoded = decodeTauriText(encoded, "updater 签名");
  const lines = decoded.split("\n");
  if (
    decoded.includes("\r") ||
    lines.length !== 4 ||
    lines.some((line) => /\s$/.test(line))
  ) {
    throw new Error("updater 签名 inner text 必须精确四行且 comment 无尾随空白");
  }
  const packet = decodeCanonicalBase64(lines[1] ?? "", "updater 签名 packet");
  const global = decodeCanonicalBase64(lines[3] ?? "", "updater 全局签名");
  const algorithm = packet.subarray(0, 2).toString("ascii");
  if (
    !lines[2].startsWith("trusted comment: ") ||
    packet.length !== 74 ||
    global.length !== 64 ||
    algorithm !== "ED"
  ) {
    throw new Error("updater 签名必须使用 prehashed ED 模式");
  }
  return {
    algorithm,
    id: packet.subarray(2, 10),
    signature: packet.subarray(10),
    comment: lines[2].slice("trusted comment: ".length),
    global,
  };
}

/** 把 Tauri signer 唯一允许的末尾 LF 收敛为公开资产使用的 canonical 四行签名。 */
export function normalizeTauriUpdaterSignature(encoded) {
  const decoded = decodeTauriText(encoded, "updater 签名");
  const normalized = decoded.endsWith("\n") && !decoded.endsWith("\n\n")
    ? decoded.slice(0, -1)
    : decoded;
  const canonical = Buffer.from(normalized, "utf8").toString("base64");
  parseUpdaterSignature(canonical);
  return canonical;
}

/** 使用 Node 内置 Ed25519 验证 Tauri updater 包与可信注释。 */
export function verifyTauriUpdaterSignature(packageBytes, signatureText, publicKeyText) {
  const publicKey = parsePublicKey(publicKeyText);
  const signature = parseUpdaterSignature(signatureText);
  if (!publicKey.id.equals(signature.id)) {
    throw new Error("updater 签名 key id 不匹配");
  }
  const key = createPublicKey({
    key: Buffer.concat([
      Buffer.from("302a300506032b6570032100", "hex"),
      publicKey.key,
    ]),
    format: "der",
    type: "spki",
  });
  const message = createHash("blake2b512").update(packageBytes).digest();
  if (!verifySignature(null, message, key, signature.signature)) {
    throw new Error("updater 包签名无效");
  }
  const globalMessage = Buffer.concat([
    signature.signature,
    Buffer.from(signature.comment),
  ]);
  if (!verifySignature(null, globalMessage, key, signature.global)) {
    throw new Error("updater 全局签名无效");
  }
}

/** 校验 canonical updater public key，并返回可持久化而不泄露原文的 SHA-256 身份。 */
export function updaterPublicKeySha256(publicKeyText) {
  parsePublicKey(publicKeyText);
  return createHash("sha256").update(publicKeyText).digest("hex");
}

/** normal 必须绑定 previous archive key；bootstrap 必须完全省略该历史输入。 */
export function resolvePreviousReleaseSigningKeySha256(mode, publicKeyText) {
  if (mode === "bootstrap") {
    if (publicKeyText) throw new Error("bootstrap 不得提供 previous release key");
    return undefined;
  }
  if (mode !== "normal" || !publicKeyText) throw new Error("normal 缺少 previous release key");
  return updaterPublicKeySha256(publicKeyText);
}

/**
 * 解析本版包签名 key 与候选内嵌 successor key；常规发布必须相同，只有 exact normal bridge tag 可不同。
 * 返回值只包含 SHA-256 身份，避免把完整公钥写入 receipt、artifact 或日志。
 */
export function resolveUpdaterKeyPolicy(options) {
  const {
    tag,
    mode = "normal",
    releaseSigningPublicKey,
    candidateUpdaterPublicKey,
    bridgeTag = "",
  } = options;
  parseReleaseIdentity(tag, tag.replace(/^v/, ""));
  if (!["normal", "bootstrap"].includes(mode)) throw new Error("updater key policy mode 无效");
  const releaseSigningKeySha256 = updaterPublicKeySha256(releaseSigningPublicKey);
  const candidateUpdaterKeySha256 = updaterPublicKeySha256(candidateUpdaterPublicKey);
  const differs = releaseSigningKeySha256 !== candidateUpdaterKeySha256;
  if (differs && mode !== "normal") {
    throw new Error("bootstrap 不允许 updater key bridge");
  }
  if (differs && bridgeTag !== tag) {
    throw new Error("updater key bridge 缺少 exact tag 授权");
  }
  return Object.freeze({
    releaseSigningKeySha256,
    candidateUpdaterKeySha256,
    bridgeAuthorized: differs,
  });
}

/** 要求资产列表与期望集合完全相等，并单独报告重复、缺少和意外名称。 */
export function assertExactNameSet(actualNames, expectedNames, label) {
  const counts = new Map();
  for (const name of actualNames) {
    counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  const duplicates = [...counts].filter(([, count]) => count > 1).map(([name]) => name);
  const expected = new Set(expectedNames);
  const missing = expectedNames.filter((name) => !counts.has(name));
  const unexpected = [...counts.keys()].filter((name) => !expected.has(name));
  if (duplicates.length || missing.length || unexpected.length) {
    throw new Error([
      `${label}不精确`,
      duplicates.length ? `重复：${duplicates.sort().join(", ")}` : "",
      missing.length ? `缺少：${missing.sort().join(", ")}` : "",
      unexpected.length ? `意外：${unexpected.sort().join(", ")}` : "",
    ].filter(Boolean).join("；"));
  }
}

/** 以最小魔数与容器头验证公开安装包/更新包没有被错配为普通文件。 */
function assertNativeContainer(name, bytes) {
  if (name.endsWith(".dmg")) {
    if (bytes.length < 512 || bytes.subarray(bytes.length - 512, bytes.length - 508).toString("ascii") !== "koly") {
      throw new Error(`${name} 不是有效 UDIF DMG 容器`);
    }
    return;
  }
  if (name.endsWith(".exe")) {
    const peOffset = bytes.length >= 64 ? bytes.readUInt32LE(60) : -1;
    if (
      bytes.subarray(0, 2).toString("ascii") !== "MZ" ||
      peOffset < 0 ||
      peOffset + 4 > bytes.length ||
      !bytes.subarray(peOffset, peOffset + 4).equals(Buffer.from("PE\0\0"))
    ) {
      throw new Error(`${name} 不是有效 PE 容器`);
    }
    return;
  }
  if (name.endsWith(".app.tar.gz")) {
    let archive;
    try {
      archive = gunzipSync(bytes);
    } catch {
      throw new Error(`${name} 不是有效 gzip 容器`);
    }
    const firstPath = archive.subarray(0, 100).toString("utf8").replace(/\0.*$/, "");
    const tarMagic = archive.subarray(257, 262).toString("ascii");
    if (!tarMagic.startsWith("ustar") || !firstPath.includes(".app")) {
      throw new Error(`${name} 不是包含 macOS app 的 tar 容器`);
    }
    return;
  }
  throw new Error(`无法识别原生包容器：${name}`);
}

/** 校验 manifest 的版本、两目标、URL、名称与字面签名映射。 */
function assertManifest(manifest, { tag, version, notes, names, signatures }) {
  if (manifest.version !== version || manifest.notes !== notes || Object.hasOwn(manifest, "pub_date")) {
    throw new Error("latest.json 版本错误或 Draft 不应包含 pub_date");
  }
  assertExactNameSet(Object.keys(manifest.platforms ?? {}), TARGETS, "manifest 目标集合");
  const mappings = [
    ["darwin-aarch64", names.updater.darwin],
    ["windows-x86_64", names.updater.windows],
  ];
  for (const [target, assetName] of mappings) {
    const entry = manifest.platforms[target];
    if (
      entry.url !== publicDownloadUrl(tag, assetName) ||
      entry.signature !== signatures.get(`${assetName}.sig`)
    ) {
      throw new Error(`latest.json 的 ${target} URL/名称/版本/签名映射无效`);
    }
  }
}

/** 对外验证一个候选 updater manifest 的完整版本、目标和字面签名契约。 */
export function validateUpdaterManifest(manifest, options) {
  const { tag, version, notes, signatures } = options;
  parseReleaseIdentity(tag, version);
  const names = releaseAssetNames(version);
  const signatureMap = signatures instanceof Map
    ? signatures
    : new Map(Object.entries(signatures ?? {}));
  assertManifest(manifest, { tag, version, notes, names, signatures: signatureMap });
}

/** 开源发布元数据必须有标题与说明；源码链接和提交身份可以公开。 */
export function assertPublicReleaseMetadata({ title, notes }) {
  if (typeof title !== "string" || !title.trim() || /[\r\n\0]/.test(title)) {
    throw new Error("Release 标题必须是非空单行文本");
  }
  if (typeof notes !== "string" || !notes.trim() || notes.includes("\0")) {
    throw new Error("Release Notes 必须是非空文本");
  }
}

/** 生成稳定排序、只覆盖七个非校验和文件的 SHA256SUMS。 */
function createChecksums(assets, expectedNames) {
  assertExactNameSet(assets.map(({ name }) => name), expectedNames, "校验和输入资产集合");
  return [...assets]
    .sort((left, right) => comparePublicNames(left.name, right.name))
    .map(({ name, bytes }) => `${createHash("sha256").update(bytes).digest("hex")}  ${name}\n`)
    .join("");
}

/** 从目录读取普通文件，目录和符号链接都不能伪装成发布资产。 */
function readAssetDirectory(directory) {
  return readdirSync(directory, { withFileTypes: true }).map((entry) => {
    if (!entry.isFile()) {
      throw new Error(`发布资产必须是普通文件：${entry.name}`);
    }
    return { name: entry.name, bytes: readFileSync(join(directory, entry.name)) };
  });
}

/**
 * 验证六个原生产物，生成 deterministic manifest/checksums，再复核最终八文件 Draft。
 * 调用成功意味着目录可以交给 Draft 协调模块；失败时不会产生 SHA256SUMS。
 */
export function prepareDraftAssetDirectory(options) {
  const {
    directory,
    tag,
    version,
    notes,
    releaseSigningPublicKey,
    candidateUpdaterPublicKey,
    bridgeTag,
  } = options;
  parseReleaseIdentity(tag, version);
  const updaterTrust = resolveUpdaterKeyPolicy({
    tag,
    releaseSigningPublicKey,
    candidateUpdaterPublicKey,
    bridgeTag,
  });
  if (!String(notes ?? "").trim()) {
    throw new Error("公开 Release Notes 不能为空");
  }
  const names = releaseAssetNames(version);
  const nativeNames = [
    names.installer.darwin,
    names.installer.windows,
    names.updater.darwin,
    `${names.updater.darwin}.sig`,
    names.updater.windows,
    `${names.updater.windows}.sig`,
  ];
  const existing = readAssetDirectory(directory);
  assertExactNameSet(existing.map(({ name }) => name), nativeNames, "原生产物集合");
  const byName = new Map(existing.map((asset) => [asset.name, asset]));
  for (const name of [
    names.installer.darwin,
    names.installer.windows,
    names.updater.darwin,
    names.updater.windows,
  ]) {
    assertNativeContainer(name, byName.get(name).bytes);
  }
  const signatures = new Map([
    [`${names.updater.darwin}.sig`, normalizeTauriUpdaterSignature(byName.get(`${names.updater.darwin}.sig`).bytes.toString("utf8"))],
    [`${names.updater.windows}.sig`, normalizeTauriUpdaterSignature(byName.get(`${names.updater.windows}.sig`).bytes.toString("utf8"))],
  ]);
  for (const [name, signature] of signatures) {
    // 公开 `.sig`、latest.json 与 SHA256SUMS 必须共享同一份 canonical bytes。
    writeFileSync(join(directory, name), signature);
  }
  verifyTauriUpdaterSignature(
    byName.get(names.updater.darwin).bytes,
    signatures.get(`${names.updater.darwin}.sig`),
    releaseSigningPublicKey,
  );
  verifyTauriUpdaterSignature(
    byName.get(names.updater.windows).bytes,
    signatures.get(`${names.updater.windows}.sig`),
    releaseSigningPublicKey,
  );

  const manifest = {
    version,
    notes,
    platforms: {
      "darwin-aarch64": {
        signature: signatures.get(`${names.updater.darwin}.sig`),
        url: publicDownloadUrl(tag, names.updater.darwin),
      },
      "windows-x86_64": {
        signature: signatures.get(`${names.updater.windows}.sig`),
        url: publicDownloadUrl(tag, names.updater.windows),
      },
    },
  };
  assertManifest(manifest, { tag, version, notes, names, signatures });
  const manifestText = `${JSON.stringify(manifest, null, 2)}\n`;
  assertPublicReleaseMetadata({
    title: `Narrant ${version}`,
    notes,
    manifestText,
    assetNames: [...nativeNames, names.manifest, names.checksums],
  });
  writeFileSync(join(directory, names.manifest), manifestText);

  const sevenNames = [...nativeNames, names.manifest];
  const sevenAssets = readAssetDirectory(directory);
  const checksums = createChecksums(sevenAssets, sevenNames);
  writeFileSync(join(directory, names.checksums), checksums);
  const finalAssets = readAssetDirectory(directory);
  const finalNames = [...sevenNames, names.checksums];
  assertExactNameSet(finalAssets.map(({ name }) => name), finalNames, "最终 Draft 资产集合");
  return {
    title: `Narrant ${version}`,
    names: [...finalNames].sort(comparePublicNames),
    manifest,
    checksums,
    updaterTrust,
  };
}

/** 要求传给 Draft 协调模块的路径恰好对应当前版本八个公开名称。 */
export function assertDraftArtifactPaths(paths, version) {
  const names = releaseAssetNames(version);
  const expected = [
    names.installer.darwin,
    names.installer.windows,
    names.updater.darwin,
    `${names.updater.darwin}.sig`,
    names.updater.windows,
    `${names.updater.windows}.sig`,
    names.manifest,
    names.checksums,
  ];
  assertExactNameSet(paths.map((path) => basename(path)), expected, "上传资产集合");
}

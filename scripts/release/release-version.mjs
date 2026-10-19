import { readFileSync } from "node:fs";
import { resolve } from "node:path";

export const VERSION_PATTERN = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
export const VERSION_FILES = [
  "package.json",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
];

/** 从 Cargo TOML 的指定表中读取版本，避免误取依赖版本。 */
function readTomlTableVersion(content, tableName) {
  const table = content.match(new RegExp(`\\[${tableName}\\]([\\s\\S]*?)(?=\\n\\[|$)`));
  return table?.[1].match(/^version\s*=\s*"([^"]+)"/m)?.[1];
}

/** 从 Cargo.lock 中读取 Narrant 根包的版本。 */
function readLockVersion(content) {
  const packageBlock = content
    .split("[[package]]")
    .find((block) => /^\s*name\s*=\s*"narrant"\s*$/m.test(block));
  return packageBlock?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
}

/** 读取发布流程维护的全部版本文件原文。 */
export function readVersionFileContents(repository) {
  return new Map(
    VERSION_FILES.map((path) => [path, readFileSync(resolve(repository, path), "utf8")]),
  );
}

/** 从四份文件原文读取各自声明的应用版本。 */
export function readDeclaredVersions(contents) {
  return new Map([
    ["package.json", JSON.parse(contents.get("package.json")).version],
    [
      "src-tauri/tauri.conf.json",
      JSON.parse(contents.get("src-tauri/tauri.conf.json")).version,
    ],
    [
      "src-tauri/Cargo.toml",
      readTomlTableVersion(contents.get("src-tauri/Cargo.toml"), "package"),
    ],
    ["src-tauri/Cargo.lock", readLockVersion(contents.get("src-tauri/Cargo.lock"))],
  ]);
}

/** 要求四份版本声明一致，并返回唯一版本。 */
export function readCurrentVersion(contents) {
  const versions = [...readDeclaredVersions(contents).values()];
  if (versions.some((version) => version === undefined) || new Set(versions).size !== 1) {
    throw new Error(`当前版本声明不一致：${versions.join(", ")}`);
  }
  return versions[0];
}

/** 比较两个已经验证过的稳定 SemVer。 */
export function compareVersions(left, right) {
  const leftParts = left.split(".").map(Number);
  const rightParts = right.split(".").map(Number);
  for (let index = 0; index < leftParts.length; index += 1) {
    if (leftParts[index] !== rightParts[index]) {
      return leftParts[index] - rightParts[index];
    }
  }
  return 0;
}

/** 要求全部版本声明与标签版本相同，并给出逐文件错误。 */
export function assertDeclaredVersion(contents, expectedVersion) {
  const mismatches = [...readDeclaredVersions(contents)].filter(
    ([, actual]) => actual !== expectedVersion,
  );
  if (mismatches.length === 0) {
    return;
  }
  const details = mismatches
    .map(([path, actual]) => `${path}: 期望 ${expectedVersion}，实际 ${actual ?? "无法读取"}`)
    .join("\n");
  throw new Error(details);
}

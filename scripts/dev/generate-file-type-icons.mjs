import { cp, mkdir, rm, stat, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { generateManifest } from "material-icon-theme";

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const outputDirectory = resolve(projectRoot, "public/file-type-icons");
const manifestOutput = resolve(projectRoot, "src/lib/file-type-icon-manifest.ts");
const packageDirectory = dirname(fileURLToPath(import.meta.resolve("material-icon-theme/package.json")));
const sourceIconsDirectory = process.env.FILE_TYPE_ICON_SOURCE_DIRECTORY ?? resolve(packageDirectory, "icons");
const configuredMaximumIconBytes = Number(process.env.FILE_TYPE_ICON_MAX_BYTES);
const maximumIconBytes = Number.isFinite(configuredMaximumIconBytes) ? configuredMaximumIconBytes : 2 * 1024 * 1024;

/** 返回主题 manifest 在明暗模式下可能解析出的去重图标名称。 */
function collectIconNames(manifest) {
  const names = new Set();
  /** 收集单个明暗主题层内的默认图标与全部路径关联目标。 */
  const collect = (candidate) => {
    if (!candidate) return;
    [candidate.file, candidate.folder, candidate.folderExpanded, candidate.rootFolder, candidate.rootFolderExpanded]
      .filter(Boolean)
      .forEach((name) => names.add(name));
    Object.values(candidate.fileNames ?? {}).forEach((name) => names.add(name));
    Object.values(candidate.fileExtensions ?? {}).forEach((name) => names.add(name));
    Object.values(candidate.folderNames ?? {}).forEach((name) => names.add(name));
    Object.values(candidate.folderNamesExpanded ?? {}).forEach((name) => names.add(name));
    Object.values(candidate.rootFolderNames ?? {}).forEach((name) => names.add(name));
    Object.values(candidate.rootFolderNamesExpanded ?? {}).forEach((name) => names.add(name));
  };
  collect(manifest);
  collect(manifest.light);
  return [...names].sort();
}

/** 将 manifest 序列化为浏览器可直接使用的确定性匹配表。 */
function serializeManifest(manifest) {
  /** 合并默认主题与局部 light override，保留未覆盖的目录和文件关联。 */
  const pick = (base = {}, override = {}) => ({
    file: override.file ?? base.file ?? "file",
    folder: override.folder ?? base.folder ?? "folder",
    folderExpanded: override.folderExpanded ?? base.folderExpanded ?? "folder-open",
    fileNames: { ...(base.fileNames ?? {}), ...(override.fileNames ?? {}) },
    fileExtensions: { ...(base.fileExtensions ?? {}), ...(override.fileExtensions ?? {}) },
    folderNames: { ...(base.folderNames ?? {}), ...(override.folderNames ?? {}) },
    folderNamesExpanded: { ...(base.folderNamesExpanded ?? {}), ...(override.folderNamesExpanded ?? {}) },
  });
  return `/** 此文件由 scripts/dev/generate-file-type-icons.mjs 生成，请勿手工编辑。 */\nexport const fileTypeIconManifest = ${JSON.stringify({ dark: pick(manifest), light: pick(manifest, manifest.light) }, null, 2)} as const;\n`;
}

const manifest = generateManifest({ activeIconPack: "none" });
const iconNames = collectIconNames(manifest);
await rm(outputDirectory, { recursive: true, force: true });
await mkdir(outputDirectory, { recursive: true });

let totalBytes = 0;
for (const iconName of iconNames) {
  const iconPath = manifest.iconDefinitions?.[iconName]?.iconPath;
  const source = iconPath ? resolve(sourceIconsDirectory, iconPath.replace("./../icons/", "")) : undefined;
  const destination = resolve(outputDirectory, `${iconName}.svg`);
  const sourceStat = source ? await stat(source).catch(() => undefined) : undefined;
  if (!sourceStat?.isFile()) throw new Error(`Missing Material Icon Theme asset: ${iconName}.svg`);
  totalBytes += sourceStat.size;
  await cp(source, destination);
}

await writeFile(manifestOutput, serializeManifest(manifest));
const summary = `File Type Icons: ${iconNames.length} SVGs, ${totalBytes} raw bytes`;
process.stdout.write(`${summary}\n`);
if (totalBytes > maximumIconBytes) process.stderr.write(`WARNING: ${summary} exceeds the 2 MiB budget.\n`);

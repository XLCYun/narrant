import { fileTypeIconManifest } from "@/lib/file-type-icon-manifest";

/** 文件与目录图标解析可接受的节点类型。 */
export type FileTypeIconKind = "file" | "directory";

/** 应用当前支持的文件图标主题变体。 */
export type FileTypeIconTheme = "light" | "dark";

/** 解析 File Type Icon 所需的稳定输入。 */
export type FileTypeIconRequest = {
  path: string;
  kind: FileTypeIconKind;
  expanded?: boolean;
  theme: FileTypeIconTheme;
};

/** File Type Icon 的本地资源标识。 */
export type FileTypeIcon = { name: string };

/** 运行时 resolver 使用的主题关联表形状。 */
type FileTypeIconThemeManifest = {
  file: string;
  folder: string;
  folderExpanded: string;
  fileNames: Record<string, string>;
  fileExtensions: Record<string, string>;
  folderNames: Record<string, string>;
  folderNamesExpanded: Record<string, string>;
};

/** 返回路径最后一段，并以主题的大小写无关关联规则规范化。 */
function basename(path: string | undefined) {
  return path?.split("/").filter(Boolean).at(-1)?.toLowerCase() ?? "";
}

/** 按最长复合扩展名解析文件图标，避免把 .test.ts 错认为普通 .ts。 */
function resolveExtensionIcon(name: string, extensions: Record<string, string>) {
  return Object.entries(extensions)
    .sort(([left], [right]) => right.length - left.length)
    .find(([extension]) => name.endsWith(`.${extension}`))?.[1];
}

/** 使用构建期生成的 Material Icon Theme manifest 解析一个 File Type Icon。 */
export function resolveFileTypeIcon(request: FileTypeIconRequest): FileTypeIcon {
  const manifest = fileTypeIconManifest[request.theme] as FileTypeIconThemeManifest;
  const name = basename(request.path);
  if (request.kind === "directory") {
    const associations = request.expanded ? manifest.folderNamesExpanded : manifest.folderNames;
    return { name: associations[name] ?? (request.expanded ? manifest.folderExpanded : manifest.folder) };
  }
  return { name: manifest.fileNames[name] ?? resolveExtensionIcon(name, manifest.fileExtensions) ?? manifest.file };
}

/** 返回由 Vite public assets 提供、可随 Tauri 离线打包的图标地址。 */
export function fileTypeIconUrl(icon: FileTypeIcon) {
  return `${import.meta.env.BASE_URL}file-type-icons/${icon.name}.svg`;
}

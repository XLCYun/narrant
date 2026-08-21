import type { FileChangeContent } from "@/review/types";

/** Babysit 首批后端白名单所使用的 canonical language id。 */
export type BabysitDetectedLanguage = "typescript" | "javascript" | "rust" | "python" | "go" | "java" | "c" | "cpp";

const extensionLanguages: Readonly<Record<string, BabysitDetectedLanguage>> = {
  ts: "typescript", tsx: "typescript", mts: "typescript", js: "javascript", jsx: "javascript", mjs: "javascript",
  rs: "rust", py: "python", go: "go", java: "java", c: "c",
  cc: "cpp", cpp: "cpp", cxx: "cpp", h: "cpp", hpp: "cpp",
};

/** 从路径或可靠 shebang 检测首批语言；不把未知内容猜成可分析代码。 */
export function detectBabysitLanguage(path: string, content: FileChangeContent | undefined): BabysitDetectedLanguage | undefined {
  if (!content || content.contentKind !== "text") return undefined;
  const basename = path.replaceAll("\\", "/").split("/").at(-1) ?? "";
  const extension = basename.includes(".") ? basename.split(".").at(-1)?.toLowerCase() : undefined;
  if (extension && extensionLanguages[extension]) return extensionLanguages[extension];
  const firstLine = (content.after ?? content.before ?? "").split(/\r?\n/, 1)[0]?.trim() ?? "";
  if (/^#!.*\bpython(?:3(?:\.\d+)?)?\b/i.test(firstLine)) return "python";
  if (/^#!.*(?:\/|\s)node(?:\s|$)/i.test(firstLine)) return "javascript";
  return undefined;
}

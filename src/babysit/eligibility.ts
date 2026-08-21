import type { FileChangeContent } from "@/review/types";

/** Review Reader 在启动分析前可以向用户解释的文件 eligibility 原因。 */
export type BabysitEligibilityReason =
  | "missing-content"
  | "binary"
  | "submodule"
  | "symlink"
  | "generated"
  | "dependency"
  | "lock"
  | "minified"
  | "snapshot"
  | "unsupported-extension";

/** File Change header 使用的稳定 eligibility 结果。 */
export type BabysitEligibility = {
  eligible: boolean;
  reason?: BabysitEligibilityReason;
};

const supportedExtensions = new Set([
  "ts", "tsx", "js", "jsx", "rs", "py", "go", "java", "c", "cc", "cpp", "cxx", "h", "hpp",
]);

/** 在前端复制后端的首批代码白名单，避免不可分析文件消耗一次 Agent 请求。 */
export function getBabysitEligibility(
  path: string,
  content?: Pick<FileChangeContent, "contentKind">,
): BabysitEligibility {
  if (!content) return { eligible: false, reason: "missing-content" };
  if (content.contentKind !== "text") return { eligible: false, reason: content.contentKind };

  const normalized = path.replaceAll("\\", "/");
  const components = normalized.split("/");
  if (components.some((component) => ["node_modules", "vendor", "dist", "build", "target"].includes(component))) {
    return { eligible: false, reason: "dependency" };
  }
  if (components.some((component) => ["generated", "docs", "documentation"].includes(component.toLowerCase()))) {
    return { eligible: false, reason: "generated" };
  }

  const basename = components.at(-1)?.toLowerCase() ?? "";
  if (
    basename.endsWith(".lock")
    || ["cargo.lock", "package-lock.json", "yarn.lock", "pnpm-lock.yaml"].includes(basename)
  ) return { eligible: false, reason: "lock" };
  if (basename.endsWith(".min.js") || basename.endsWith(".min.ts")) {
    return { eligible: false, reason: "minified" };
  }
  if (basename.endsWith(".snap") || basename.endsWith(".snapshot")) {
    return { eligible: false, reason: "snapshot" };
  }

  const extension = basename.split(".").at(-1);
  return extension && supportedExtensions.has(extension)
    ? { eligible: true }
    : { eligible: false, reason: "unsupported-extension" };
}

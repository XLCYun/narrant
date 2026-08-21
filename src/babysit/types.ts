import type { SupportedLocale } from "@/lib/i18n";
import type { ReviewIdentity } from "@/review/types";

/** 与 Rust BabysitProgress 对齐的 provider-independent 进度事件。 */
export type BabysitProgress =
  | { phase: "generating" }
  | { phase: "repairing"; attempt: number }
  | { phase: "completed" };

/** 一次 File Change 的完整 before/after Line Explanation 结果。 */
export type BabysitFileExplanations = {
  before: Record<string, string>;
  after: Record<string, string>;
};

/** 绑定精确 Review Snapshot、File Change path 与界面语言的请求 identity。 */
export type BabysitFileRequest = ReviewIdentity & {
  path: string;
  locale: SupportedLocale;
};

/** 当前 Review Reader 选中文件的稳定 Babysit 运行投影。 */
export type BabysitState =
  | { kind: "idle" }
  | { kind: "generating"; request: BabysitFileRequest }
  | { kind: "repairing"; request: BabysitFileRequest; attempt: number }
  | { kind: "completed"; request: BabysitFileRequest; explanations: BabysitFileExplanations }
  | { kind: "failed"; request: BabysitFileRequest; detail: string; retryable: true };

/** 前端 lifecycle 与任意 Installed/Built-in provider adapter 之间的单一 seam。 */
export interface BabysitApplicationAdapter {
  explainFile(
    request: BabysitFileRequest,
    onProgress: (progress: BabysitProgress) => void,
    signal: AbortSignal,
  ): Promise<BabysitFileExplanations>;
}

/** Review Reader 消费的最小 Babysit action binding。 */
export type BabysitBinding = {
  state: BabysitState;
  enabled: boolean;
  explain(): void;
  cancel(): void;
  retry(): void;
};

import type { SupportedLocale } from "@/lib/i18n";
import type { ReviewIdentity } from "@/review/types";

/** 与 Rust BabysitProgress 对齐的 provider-independent 进度事件。 */
export type BabysitProgress =
  | { phase: "generating" }
  | { phase: "repairing"; attempt: number }
  | { phase: "completed" };

/** Tauri 与所有 Babysit provider 共用的稳定失败 code。 */
export type BabysitErrorCode =
  | "unsupported_file"
  | "invalid_provider_output"
  | "repair_exhausted"
  | "cancelled"
  | "provider"
  | "progress_closed"
  | "cache_unavailable"
  | "input_unavailable"
  | "invalid_request"
  | "operation_conflict"
  | "operation_unavailable";

/** Babysit command 失败的结构化 payload；detail 不参与业务分支。 */
export type BabysitError = {
  code: BabysitErrorCode;
  detail: string;
};

/** 判断 Tauri reject 值是否符合 provider-independent Babysit error shape。 */
export function isBabysitError(value: unknown): value is BabysitError {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<BabysitError>;
  return typeof candidate.code === "string" && typeof candidate.detail === "string";
}

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
  | { kind: "failed"; request: BabysitFileRequest; error: BabysitError; retryable: true };

/** 判断当前 Babysit 是否仍在 provider 生成或 repair 阶段。 */
export function isBabysitRunning(
  state: BabysitState,
): state is Extract<BabysitState, { kind: "generating" | "repairing" }> {
  return state.kind === "generating" || state.kind === "repairing";
}

/** 前端 lifecycle 与任意 Installed/Built-in provider adapter 之间的单一 seam。 */
export interface BabysitApplicationAdapter {
  /** 按完整 File Change 发起一次分析；失败时 reject 结构化 BabysitError。 */
  explainFile(
    request: BabysitFileRequest,
    onProgress: (progress: BabysitProgress) => void,
    signal: AbortSignal,
  ): Promise<BabysitFileExplanations>;
}

/** Review Reader 消费的最小 Babysit action binding。 */
export type BabysitBinding = {
  /** 当前分析阶段或已完成/失败结果。 */
  state: BabysitState;
  /** 当前选中文件是否满足 adapter 与文本 eligibility 的最小前置条件。 */
  enabled: boolean;
  /** 启动当前 File Change 的完整分析。 */
  explain(): void;
  /** 取消当前分析并丢弃未完成结果。 */
  cancel(): void;
  /** 重试最后一次失败的分析。 */
  retry(): void;
};

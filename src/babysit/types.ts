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
  | "operation_unavailable"
  | "provider_not_found"
  | "provider_executable_invalid"
  | "provider_signed_out"
  | "provider_incompatible"
  | "provider_initialization_failed"
  | "codex_not_found"
  | "codex_executable_invalid"
  | "codex_signed_out"
  | "codex_incompatible"
  | "codex_initialization_failed";

/** provider-aware setup failure 的恢复动作，不包含路径或原始进程输出。 */
export type BabysitRecoveryAction =
  | "login_provider"
  | "upgrade_provider"
  | "select_provider_executable"
  | "login_codex"
  | "upgrade_codex"
  | "select_codex_executable"
  | "retry";

/** Babysit command 失败的结构化 payload；detail 不参与业务分支。 */
export type BabysitError = {
  code: BabysitErrorCode;
  detail: string;
  /** setup failure 所属 provider；普通分析错误不携带该字段。 */
  providerId?: string;
  recovery?: BabysitRecoveryAction;
};

/** 运行时校验使用的公开 Babysit error code 白名单。 */
const babysitErrorCodes: ReadonlySet<BabysitErrorCode> = new Set([
  "unsupported_file",
  "invalid_provider_output",
  "repair_exhausted",
  "cancelled",
  "provider",
  "progress_closed",
  "cache_unavailable",
  "input_unavailable",
  "invalid_request",
  "operation_conflict",
  "operation_unavailable",
  "provider_not_found",
  "provider_executable_invalid",
  "provider_signed_out",
  "provider_incompatible",
  "provider_initialization_failed",
  "codex_not_found",
  "codex_executable_invalid",
  "codex_signed_out",
  "codex_incompatible",
  "codex_initialization_failed",
]);
const babysitRecoveryActions: ReadonlySet<BabysitRecoveryAction> = new Set([
  "login_provider",
  "upgrade_provider",
  "select_provider_executable",
  "login_codex",
  "upgrade_codex",
  "select_codex_executable",
  "retry",
]);

/** 判断 Tauri reject 值是否符合 provider-independent Babysit error shape。 */
export function isBabysitError(value: unknown): value is BabysitError {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<BabysitError>;
  return typeof candidate.code === "string"
    && babysitErrorCodes.has(candidate.code as BabysitErrorCode)
    && typeof candidate.detail === "string"
    && (candidate.providerId === undefined || typeof candidate.providerId === "string")
    && (candidate.recovery === undefined
      || (typeof candidate.recovery === "string"
        && babysitRecoveryActions.has(candidate.recovery as BabysitRecoveryAction)));
}

/** 一次 File Change 的完整 before/after Line Explanation 结果。 */
export type BabysitFileExplanations = {
  before: Record<string, string>;
  after: Record<string, string>;
};

/** 控制同 identity 的分析复用现有结果，或明确请求一套新解释。 */
export type BabysitGenerationPolicy = "reuse_cached" | "regenerate";

/** 绑定精确 Review Snapshot、File Change path 与界面语言的请求 identity。 */
export type BabysitFileRequest = ReviewIdentity & {
  path: string;
  locale: SupportedLocale;
  detectedLanguage?: string;
};

/** 一次 Babysit 分析调用，组合固定文件 identity 与本次生成策略。 */
export type BabysitAnalysisRequest = {
  file: BabysitFileRequest;
  generationPolicy: BabysitGenerationPolicy;
};

/** 后端唯一判定的文件 eligibility reason；UI 只负责本地化。 */
export type BabysitEligibilityReason =
  | "non_text"
  | "generated_or_vendored"
  | "lock_minified_or_snapshot"
  | "unsupported_language";

/** 后端返回的 provider-independent eligibility 结论。 */
export type BabysitEligibility =
  | { eligible: true; reason: null }
  | { eligible: false; reason: BabysitEligibilityReason };

/** 当前文件 eligibility 的异步读取状态。 */
export type BabysitEligibilityState =
  | { kind: "checking" }
  | { kind: "resolved"; result: BabysitEligibility }
  | { kind: "unavailable" };

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
  /** 由后端读取固定 Snapshot 并给出唯一 eligibility 结论。 */
  checkEligibility(
    request: Omit<BabysitFileRequest, "locale">,
    detectedLanguage: string | undefined,
    signal: AbortSignal,
  ): Promise<BabysitEligibility>;
  /** 按完整 File Change 发起一次分析；失败时 reject 结构化 BabysitError。 */
  explainFile(
    request: BabysitAnalysisRequest,
    onProgress: (progress: BabysitProgress) => void,
    signal: AbortSignal,
  ): Promise<BabysitFileExplanations>;
  /** Snapshot 变化后删除旧结果；失败不应阻塞新的 Review Snapshot 展示。 */
  clearSnapshot?(identity: ReviewIdentity): Promise<void>;
  /** Review 关闭或切换后删除该 Review 的进程内结果。 */
  clearReview?(reviewId: string): Promise<void>;
  /** 由 native picker 选择并验证任意 installed provider executable。 */
  selectProviderExecutable?(providerId: string, path: string): Promise<void>;
  /** 兼容现有 Codex picker；新 provider 使用 selectProviderExecutable。 */
  selectCodexExecutable?(path: string): Promise<void>;
}

/** Review Reader 消费的最小 Babysit action binding。 */
export type BabysitBinding = {
  /** 当前分析阶段或已完成/失败结果。 */
  state: BabysitState;
  /** 当前选中文件是否满足 adapter 与文本 eligibility 的最小前置条件。 */
  eligibility: BabysitEligibilityState;
  /** 启动当前 File Change 的完整分析。 */
  explain(): void;
  /** 绕过当前文件缓存，重新生成并替换 Line Explanations。 */
  regenerate(): void;
  /** 取消当前分析并丢弃未完成结果。 */
  cancel(): void;
  /** 重试最后一次失败的分析。 */
  retry(): void;
  /** 执行 typed executable recovery action；取消选择不改变失败状态。 */
  selectExecutable?(): void;
};

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
  | "output_limit"
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
  | "model_preference_invalid"
  | "settings_persistence_failed";

/** Installed provider 初始化阶段共用的稳定失败 code。 */
export type BabysitProviderSetupErrorCode = Extract<BabysitErrorCode,
  | "provider_not_found"
  | "provider_executable_invalid"
  | "provider_signed_out"
  | "provider_incompatible"
  | "provider_initialization_failed"
>;

/** provider-aware setup failure 的恢复动作，不包含路径或原始进程输出。 */
export type BabysitRecoveryAction =
  | "login_provider"
  | "upgrade_provider"
  | "open_application_settings"
  | "retry";

/** Custom transport 失败允许跨 IPC 与剪贴板传递的白名单诊断。 */
export type BabysitProviderDiagnostic = {
  providerId: "custom";
  protocol: "openai_chat_completions" | "anthropic_messages";
  phase: "setup" | "model" | "response";
  status: "network" | "invalid_endpoint" | "unauthorized" | "client" | "server" | "rate_limited" | "invalid_response" | "unknown";
  reason: "model_not_found" | "reasoning_tools_incompatible" | "tools_unsupported" | "parameter_unsupported" | "error_envelope" | "model_turn_limit" | "unknown_tool_call" | "unknown";
  errorCategory: "transport";
};

/** Babysit command 失败的结构化 payload；detail 不参与业务分支。 */
export type BabysitError =
  | {
    code: BabysitProviderSetupErrorCode;
    detail: string;
    /** setup failure 必须标明所属 provider，UI 不从错误码猜测 provider。 */
    providerId: string;
    recovery?: BabysitRecoveryAction;
    diagnostic?: never;
  }
  | {
    code: Exclude<BabysitErrorCode, BabysitProviderSetupErrorCode>;
    detail: string;
    diagnostic?: BabysitProviderDiagnostic;
    providerId?: never;
    recovery?: never;
  };

/** 运行时校验使用的公开 Babysit error code 白名单。 */
const babysitErrorCodes: ReadonlySet<BabysitErrorCode> = new Set([
  "unsupported_file",
  "invalid_provider_output",
  "repair_exhausted",
  "output_limit",
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
  "model_preference_invalid",
  "settings_persistence_failed",
]);
const babysitProviderSetupErrorCodes: ReadonlySet<BabysitProviderSetupErrorCode> = new Set([
  "provider_not_found",
  "provider_executable_invalid",
  "provider_signed_out",
  "provider_incompatible",
  "provider_initialization_failed",
]);
const babysitRecoveryActions: ReadonlySet<BabysitRecoveryAction> = new Set([
  "login_provider",
  "upgrade_provider",
  "open_application_settings",
  "retry",
]);
const babysitProviderDiagnosticProtocols = new Set(["openai_chat_completions", "anthropic_messages"]);
const babysitProviderDiagnosticPhases = new Set(["setup", "model", "response"]);
const babysitProviderDiagnosticStatuses = new Set(["network", "invalid_endpoint", "unauthorized", "client", "server", "rate_limited", "invalid_response", "unknown"]);
const babysitProviderDiagnosticReasons = new Set(["model_not_found", "reasoning_tools_incompatible", "tools_unsupported", "parameter_unsupported", "error_envelope", "model_turn_limit", "unknown_tool_call", "unknown"]);

/** 验证 provider 诊断只包含固定白名单字段和值。 */
function isBabysitProviderDiagnostic(value: unknown): value is BabysitProviderDiagnostic {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return candidate.providerId === "custom"
    && typeof candidate.protocol === "string"
    && babysitProviderDiagnosticProtocols.has(candidate.protocol)
    && typeof candidate.phase === "string"
    && babysitProviderDiagnosticPhases.has(candidate.phase)
    && typeof candidate.status === "string"
    && babysitProviderDiagnosticStatuses.has(candidate.status)
    && typeof candidate.reason === "string"
    && babysitProviderDiagnosticReasons.has(candidate.reason)
    && candidate.errorCategory === "transport"
    && Object.keys(candidate).every((key) => ["providerId", "protocol", "phase", "status", "reason", "errorCategory"].includes(key));
}

/** 判断 Tauri reject 值是否符合 provider-independent Babysit error shape。 */
export function isBabysitError(value: unknown): value is BabysitError {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.code !== "string"
    || !babysitErrorCodes.has(candidate.code as BabysitErrorCode)
    || typeof candidate.detail !== "string") return false;
  const setupError = babysitProviderSetupErrorCodes.has(candidate.code as BabysitProviderSetupErrorCode);
  if (setupError) {
    return typeof candidate.providerId === "string"
      && candidate.providerId.length > 0
      && (candidate.recovery === undefined
        || (typeof candidate.recovery === "string"
          && babysitRecoveryActions.has(candidate.recovery as BabysitRecoveryAction)));
  }
  return candidate.providerId === undefined
    && candidate.recovery === undefined
    && (candidate.diagnostic === undefined
      || (candidate.code === "provider" && isBabysitProviderDiagnostic(candidate.diagnostic)));
}

/** 一次 File Change 的完整 before/after Line Explanation 结果。 */
export type BabysitFileExplanations = {
  before: Record<string, string>;
  after: Record<string, string>;
};

/** 控制同 identity 的分析复用现有结果，或明确请求一套新解释。 */
export type BabysitGenerationPolicy = "reuse_cached" | "regenerate";

/** 绑定精确 Review Snapshot 与 File Change path 的请求 identity。 */
export type BabysitFileRequest = ReviewIdentity & {
  path: string;
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
    request: Pick<BabysitFileRequest, "reviewId" | "snapshotId" | "path">,
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
};

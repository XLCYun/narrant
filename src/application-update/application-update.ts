/** Application Update 对前端公开的稳定状态集合。 */
export type ApplicationUpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "up_to_date" }
  | { kind: "available" }
  | { kind: "downloading"; progress: ApplicationUpdateProgress }
  | { kind: "staged" }
  | { kind: "failed" };

/** 下载长度可能由可信响应提供，也可能保持未知。 */
export type ApplicationUpdateProgress =
  | { kind: "unknown_total"; downloadedBytes: number }
  | { kind: "known_total"; downloadedBytes: number; totalBytes: number };

export type ApplicationUpdateFailureCategory =
  | "network" | "request_timeout" | "rate_limited" | "server"
  | "manifest" | "verification" | "storage" | "installer";

export type ApplicationUpdateFailure = {
  operation: "check" | "download";
  category: ApplicationUpdateFailureCategory;
  retryable: boolean;
  message: string;
};

export type ApplicationUpdateWarning = {
  category: ApplicationUpdateFailureCategory;
  message: string;
};

export type ApplicationUpdatePreferenceWarning = ApplicationUpdateWarning & {
  kind: "rejected" | "durability_unknown" | "state_unknown";
};

/** Rust 单一事件源发布的完整 provider-neutral 快照。 */
export type ApplicationUpdateSnapshot = {
  sequence: number;
  currentVersion: string;
  state: ApplicationUpdateState;
  automaticDownload: boolean;
  candidate: { version: string; releaseNotes: string } | null;
  failure: ApplicationUpdateFailure | null;
  recoveryWarning: ApplicationUpdateWarning | null;
  preferenceWarning: ApplicationUpdatePreferenceWarning | null;
};

/** 前端 controller 只允许发送无信任输入的用户意图。 */
export interface ApplicationUpdateApplication {
  snapshot(): Promise<ApplicationUpdateSnapshot>;
  subscribe(listener: (snapshot: ApplicationUpdateSnapshot) => void): Promise<() => void>;
  manualCheck(): Promise<ApplicationUpdateSnapshot>;
  setAutomaticDownload(enabled: boolean): Promise<ApplicationUpdateSnapshot>;
  retry(): Promise<ApplicationUpdateSnapshot>;
  restart(): Promise<ApplicationUpdateSnapshot>;
}

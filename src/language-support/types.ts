/** Rust 独立成包，TypeScript 和 JavaScript 共用语言服务。 */
export type PackId = "rust" | "typescript-javascript";
/** 阶段进度保留稳定终态，不把下载完成误认为安装完成。 */
export type PackPhase = "not-installed" | "downloading" | "verifying" | "installing" | "ready" | "failed";
/** 界面所需的语言包信息，不接收可执行路径或任意远端地址。 */
export type PackSnapshot = {
  id: PackId; version: string; target: string; required: boolean; phase: PackPhase;
  downloadedBytes: number; totalBytes: number | null; installedBytes: number; inUse: boolean; error: string | null;
};
/** 应用共享的完整状态，序号用于拒绝迟到结果。 */
export type PackCollection = { sequence: number; packs: PackSnapshot[] };
/** 读取状态和明确管理操作的窄接口；查询不会启动下载。 */
export interface LanguageSupportApplication {
  snapshot(): Promise<PackCollection>;
  retry(id: PackId): Promise<PackCollection>;
  remove(id: PackId, version: string): Promise<PackCollection>;
}
/** 下载和安装尚未完成时，界面继续查询同一状态。 */
export function isPackActive(pack: PackSnapshot) { return ["downloading", "verifying", "installing"].includes(pack.phase); }

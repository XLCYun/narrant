import type { BabysitApplicationAdapter, BabysitProgress } from "@/babysit/types";
import { createTauriChannel, tauriTransport, type TypedTauriTransport } from "@/infrastructure/tauri/transport";

/** 创建只负责 Babysit contract 的 Tauri adapter，与 Review lifecycle adapter 保持分离。 */
export function createTauriBabysitApplicationAdapter(
  transport: TypedTauriTransport = tauriTransport,
  createChannel: typeof createTauriChannel = createTauriChannel,
): BabysitApplicationAdapter {
  return {
    /** 通过独立 command 读取后端唯一维护的文件 eligibility。 */
    checkEligibility(request, detectedLanguage, signal) {
      return transport.invokeCancellable("check_babysit_file_eligibility", {
        reviewId: request.reviewId,
        snapshotId: request.snapshotId,
        path: request.path,
        detectedLanguage,
      }, signal);
    },
    /** 将完整 File Change analysis 映射为带 progress channel 的 cancellable command。 */
    explainFile(request, onProgress, signal) {
      const progress = createChannel<BabysitProgress>(onProgress);
      return transport.invokeCancellable("explain_review_file_change", {
        reviewId: request.reviewId,
        snapshotId: request.snapshotId,
        path: request.path,
        locale: request.locale,
        detectedLanguage: request.detectedLanguage,
        progress,
      }, signal);
    },
  };
}

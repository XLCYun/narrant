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
    explainFile({ file, generationPolicy }, onProgress, signal) {
      const progress = createChannel<BabysitProgress>(onProgress);
      return transport.invokeCancellable("explain_review_file_change", {
        reviewId: file.reviewId,
        snapshotId: file.snapshotId,
        path: file.path,
        locale: file.locale,
        detectedLanguage: file.detectedLanguage,
        generationPolicy,
        progress,
      }, signal);
    },
    /** Snapshot 更新后清理旧 identity；清理失败不携带源文本或路径诊断。 */
    clearSnapshot(identity) {
      return transport.invoke("clear_babysit_snapshot_cache", {
        reviewId: identity.reviewId,
        snapshotId: identity.snapshotId,
      });
    },
    /** Review 关闭或离开时清理该 Review 的进程内结果。 */
    clearReview(reviewId) {
      return transport.invoke("clear_babysit_review_cache", { reviewId });
    },
    /** 将 native picker 选择交给后端验证并持久化，不把路径写入诊断。 */
  };
}

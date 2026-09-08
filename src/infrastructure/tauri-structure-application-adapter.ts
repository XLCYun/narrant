import {
  createTauriChannel,
  tauriTransport,
  type TypedTauriTransport,
} from "@/infrastructure/tauri/transport";
import type { StructureApplicationAdapter } from "@/review/structure-application";
import type { StructureEvent } from "@/review/structure-types";

/** 将结构分析与完整源码读取映射为独立、可取消的 Tauri command。 */
export function createTauriStructureApplicationAdapter(
  transport: TypedTauriTransport = tauriTransport,
  createChannel: typeof createTauriChannel = createTauriChannel,
): StructureApplicationAdapter {
  return {
    /** 进度通道由单次调用持有；运行生命周期在上层拦截过期回写。 */
    analyze(identity, onEvent, signal) {
      const progress = createChannel<StructureEvent>(onEvent);
      return transport.invokeCancellable("analyze_review_structure", {
        reviewId: identity.reviewId,
        snapshotId: identity.snapshotId,
        progress,
      }, signal);
    },
    /** 保留真实侧与路径，直接返回完整文件对及各侧不可用原因。 */
    readSource(identity, request, signal) {
      return transport.invokeCancellable("read_review_structure_source", {
        reviewId: identity.reviewId,
        snapshotId: identity.snapshotId,
        side: request.side,
        path: request.path,
      }, signal);
    },
  };
}

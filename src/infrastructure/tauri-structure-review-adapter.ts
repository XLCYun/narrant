import type { TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type { StructureReviewAdapter } from "@/structure-review/types";

/** 将结构分析意图接到共享的可取消 Tauri transport。 */
export function createTauriStructureReviewAdapter(transport: TypedTauriTransport): StructureReviewAdapter {
  return { analyze: (identity, signal) => transport.invokeCancellable("analyze_structure_review", identity, signal) };
}

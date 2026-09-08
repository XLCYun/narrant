import type {
  StructureEvent,
  StructurePeekContent,
  StructureResult,
  StructureSourceRequest,
} from "@/review/structure-types";
import type { ReviewIdentity } from "@/review/types";

/** 结构视图依赖的应用边界；分析与源码读取都绑定同一不可变 Snapshot。 */
export interface StructureApplicationAdapter {
  /** 分析完整 Snapshot，通过有序事件报告进度及可用检查点。 */
  analyze(
    identity: ReviewIdentity,
    onEvent: (event: StructureEvent) => void,
    signal: AbortSignal,
  ): Promise<StructureResult>;
  /** 按真实侧与路径读取完整文件对，不依赖已完成的分析或 File Changes 选择。 */
  readSource(
    identity: ReviewIdentity,
    request: StructureSourceRequest,
    signal: AbortSignal,
  ): Promise<StructurePeekContent>;
}

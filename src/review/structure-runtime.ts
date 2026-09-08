import type { StructureApplicationAdapter } from "@/review/structure-application";
import { createStructureGraphPresentation } from "@/review/structure-graph";
import type { StructureGraphAnalysis, StructureGraphNavigation, StructureGraphPresentation } from "@/review/structure-graph-types";
import { resolveStructurePeekNavigation, type StructurePeekState } from "@/review/structure-peek";
import type { StructurePeekIdentity, StructureResult } from "@/review/structure-types";
import type { ReviewIdentity } from "@/review/types";

/** Reader 直接消费的结构状态；源码选择和编辑器位置独立于 File Changes。 */
export type StructureReaderBinding = {
  analysis: StructureGraphAnalysis;
  presentation: StructureGraphPresentation;
  peek: StructurePeekState | null;
  peekSize: number;
  peekViewState: unknown;
  setPresentation(presentation: StructureGraphPresentation): void;
  navigate(intent: StructureGraphNavigation): void;
  cancel(): void;
  retry(): void;
  selectLocation(index: number): void;
  retrySource(): void;
  closePeek(): void;
  setPeekSize(size: number): void;
  savePeekViewState(identity: StructurePeekIdentity, state: unknown): void;
};

/** 稳定动作引用避免相机变化导致 Monaco 重复执行保存与恢复 effect。 */
type StructureReaderActions = Pick<StructureReaderBinding, "setPresentation" | "navigate" | "cancel" | "retry" | "selectLocation" | "retrySource" | "closePeek" | "setPeekSize" | "savePeekViewState">;

/** 一次请求的取消及已结算状态；generation 在重试和关闭时使全部旧回调失效。 */
type RuntimeRequest = { generation: number; abort: AbortController; settled: boolean; cancellationRequested: boolean };

/** 将可读错误保存为该可选视图的状态，不抛入 Review 刷新事务。 */
function errorMessage(reason: unknown): string {
  if (reason instanceof Error) return reason.message;
  if (typeof reason === "object" && reason && "detail" in reason && typeof reason.detail === "string") return reason.detail;
  return String(reason);
}

/** 每个打开 Review 独占一个 Snapshot runtime；所有回写验证实例、版本和请求。 */
export class StructureRuntime {
  private analysis: StructureGraphAnalysis = { status: "idle", result: null, progress: null, error: null };
  private presentation: StructureGraphPresentation;
  private peek: StructurePeekState | null = null;
  private peekSize: number;
  private readonly sourceViewStates = new Map<string, unknown>();
  private analysisRequest?: RuntimeRequest;
  private sourceRequest?: RuntimeRequest;
  private generation = 0;
  private revealRequest = 0;
  private disposed = false;

  /** 注入的 current 验证此实例仍属于 Open Review Set 的同一 Snapshot。 */
  constructor(
    readonly identity: ReviewIdentity,
    private readonly adapter: StructureApplicationAdapter,
    private readonly current: () => boolean,
    private readonly changed: () => void,
    previous?: StructureReaderBinding,
  ) {
    this.presentation = createStructureGraphPresentation();
    if (previous) this.presentation.filters = { ...previous.presentation.filters };
    this.peekSize = previous?.peekSize ?? 42;
  }

  /** 所有动作与异步路径都必须先证明仍然拥有当前 Snapshot。 */
  private available(): boolean { return !this.disposed && this.current(); }

  /** 显式取消仅接收同代次 cancelled 终态；迟到进度或成功始终拒绝。 */
  private accepts(request: RuntimeRequest, latest: RuntimeRequest | undefined, cancelledTerminal = false): boolean {
    if (!this.available() || request.generation !== latest?.generation || request.settled) return false;
    return request.abort.signal.aborted
      ? request.cancellationRequested && cancelledTerminal
      : !request.cancellationRequested;
  }

  /** 不接受其它 Review 或 Snapshot 的检查点，即使它从当前 channel 到达。 */
  private matches(result: ReviewIdentity): boolean {
    return result.reviewId === this.identity.reviewId && result.snapshotId === this.identity.snapshotId;
  }

  /** 返回可观察状态和绑定到此 Review 的动作，切换展示不会改写所属对象。 */
  binding(): StructureReaderBinding {
    const sourceId = this.peek?.selection.locations[this.peek.locationIndex]?.sourceId;
    return {
      analysis: this.analysis, presentation: this.presentation, peek: this.peek, peekSize: this.peekSize,
      peekViewState: sourceId ? this.sourceViewStates.get(sourceId) ?? null : null,
      ...this.actions,
    };
  }

  /** 同一 Snapshot 的动作对象只创建一次，状态投影仍随更新返回。 */
  private readonly actions: StructureReaderActions = {
    setPresentation: (value) => { if (this.available()) { this.presentation = value; this.changed(); } },
    navigate: (intent) => this.navigate(intent), cancel: () => this.cancel(), retry: () => this.start(true),
    selectLocation: (index) => this.selectLocation(index), retrySource: () => this.readSource(),
    closePeek: () => { if (this.available()) { this.sourceRequest?.abort.abort(); this.peek = null; this.changed(); } },
    setPeekSize: (size) => { if (this.available() && Number.isFinite(size)) { this.peekSize = Math.max(20, Math.min(75, size)); this.changed(); } },
    savePeekViewState: (identity, state) => {
      if (this.available() && this.matches(identity) && this.analysis.result?.sources.some((source) => source.id === identity.sourceId)) this.sourceViewStates.set(identity.sourceId, state);
    },
  };

  /** 首次进入才启动；显式重试允许替换同一 Snapshot 的旧请求。 */
  start(retry = false): void {
    if (!this.available() || (!retry && this.analysis.status !== "idle")) return;
    const previous = this.analysisRequest;
    this.analysisRequest = undefined;
    previous?.abort.abort();
    const request: RuntimeRequest = { generation: ++this.generation, abort: new AbortController(), settled: false, cancellationRequested: false };
    this.analysisRequest = request;
    this.analysis = { ...this.analysis, status: "running", progress: null, error: null };
    this.changed();
    void (async () => this.adapter.analyze(this.identity, (event) => {
      if (!this.accepts(request, this.analysisRequest, event.kind === "checkpoint" && event.result.status === "cancelled")) return;
      if (event.kind === "progress") this.analysis = { ...this.analysis, progress: event.progress };
      else {
        if (!this.matches(event.result)) return;
        this.installResult(event.result);
        if (request.cancellationRequested) {
          request.settled = true;
          this.analysis = { ...this.analysis, status: "cancelled", progress: null };
        }
      }
      this.changed();
    }, request.abort.signal))().then((result) => {
      if (!this.accepts(request, this.analysisRequest, result.status === "cancelled")) return;
      if (!this.matches(result)) throw new Error("Code structure result does not belong to the current Snapshot.");
      if (result.status === "running") throw new Error("Code structure analysis returned without a terminal status.");
      this.installResult(result);
      request.settled = true;
      this.analysis = { ...this.analysis, status: result.status, progress: null };
      this.changed();
    }).catch((reason: unknown) => {
      if (!this.accepts(request, this.analysisRequest)) return;
      request.settled = true;
      this.analysis = { ...this.analysis, status: "failed", error: errorMessage(reason), progress: null };
      this.changed();
    });
  }

  /** 首个自洽结果初始化展开容器，后续检查点保持用户相机和选择。 */
  private installResult(result: StructureResult): void {
    if (!this.analysis.result) {
      const initial = createStructureGraphPresentation(result);
      this.presentation = { ...this.presentation, expandedIds: initial.expandedIds };
    }
    this.analysis = { ...this.analysis, result };
  }

  /** 取消保留已有检查点，并等待同代次可信 cancelled 终态补充部分结果。 */
  private cancel(): void {
    if (!this.available() || this.analysis.status !== "running") return;
    if (this.analysisRequest) {
      this.analysisRequest.cancellationRequested = true;
      this.analysisRequest.abort.abort();
    }
    this.analysis = { ...this.analysis, status: "cancelled", progress: null };
    this.changed();
  }

  /** 只由 owned 结果解析真实来源，普通 Peek 导航不选择 File Change。 */
  private navigate(intent: StructureGraphNavigation): void {
    if (!this.available() || !this.analysis.result) return;
    const selection = resolveStructurePeekNavigation(this.analysis.result, intent);
    if (!selection) return;
    this.sourceRequest?.abort.abort();
    this.peek = { selection, locationIndex: 0, requestId: ++this.revealRequest, load: { status: "idle" } };
    this.readSource();
  }

  /** 切换真实来源或同文件的发生位置时重复定位，旧读取不得覆盖新选择。 */
  private selectLocation(index: number): void {
    if (!this.available() || !this.peek || !this.peek.selection.locations[index]) return;
    const oldSource = this.peek.selection.locations[this.peek.locationIndex]?.sourceId;
    const nextSource = this.peek.selection.locations[index].sourceId;
    this.peek = { ...this.peek, locationIndex: index, requestId: ++this.revealRequest };
    if (oldSource === nextSource && this.peek.load.status === "ready") this.changed();
    else this.readSource();
  }

  /** 从来源描述符请求完整真实文件对，unknown 或缺对象绝不转成空文本。 */
  private readSource(): void {
    if (!this.available() || !this.peek) return;
    this.sourceRequest?.abort.abort();
    const location = this.peek.selection.locations[this.peek.locationIndex];
    const source = this.peek.selection.sources.find((item) => item.id === location?.sourceId);
    const side = location && source?.[location.side];
    if (!location || !source || side?.state !== "present") { this.changed(); return; }
    const request: RuntimeRequest = { generation: ++this.generation, abort: new AbortController(), settled: false, cancellationRequested: false };
    this.sourceRequest = request;
    this.peek = { ...this.peek, load: { status: "loading" } };
    this.changed();
    void (async () => this.adapter.readSource(this.identity, { side: location.side, path: side.path }, request.abort.signal))().then((content) => {
      if (!this.accepts(request, this.sourceRequest) || !this.peek) return;
      if (!this.matches(content) || content.sourceId !== source.id || content.source.id !== source.id) throw new Error("Source content does not belong to the selected Snapshot file.");
      request.settled = true;
      this.peek = { ...this.peek, load: { status: "ready", content } };
      this.changed();
    }).catch((reason: unknown) => {
      if (!this.accepts(request, this.sourceRequest) || !this.peek) return;
      request.settled = true;
      this.peek = { ...this.peek, load: { status: "failed", message: errorMessage(reason), retryable: true } };
      this.changed();
    });
  }

  /** 刷新、关闭及卸载永久撤销此实例，迟到回调和旧 binding 动作都失效。 */
  dispose(): void {
    this.disposed = true;
    this.analysisRequest?.abort.abort();
    this.sourceRequest?.abort.abort();
  }
}

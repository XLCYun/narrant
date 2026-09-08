import { useEffect, useMemo, useRef, useState } from "react";
import { ReviewReader } from "@/components/review-reader";
import { Button } from "@/components/ui/button";
import { createStructurePeekPreview, type StructurePeekScenario } from "@/dev/scenarios/structure-peek-preview";
import type { ReviewApplicationAdapter } from "@/review/review-application";
import type { StructureApplicationAdapter } from "@/review/structure-application";
import type { StructurePeekContent } from "@/review/structure-types";
import { useReviewLifecycle } from "@/review/use-review-lifecycle";
import type { FileChangeContent, ReviewSnapshot } from "@/review/types";

/** 全 Reader 验收场景覆盖真实生命周期、上下分割和同级视图导航。 */
export type ReviewStructureScenario = "changed" | "deleted" | "moved" | "renamed" | "unchanged" | "unavailable" | "long" | "loading" | "file-added" | "file-deleted" | "empty-file" | "long-path" | "multiple-impls" | "multiple-occurrences";

/** 只为 Harness 制造可取消延迟，不访问 Tauri、Git、文件系统或业务数据。 */
function previewDelay(signal: AbortSignal, duration: number): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) { reject(new DOMException("Cancelled", "AbortError")); return; }
    const abort = () => { clearTimeout(timer); signal.removeEventListener("abort", abort); reject(new DOMException("Cancelled", "AbortError")); };
    const timer = setTimeout(() => { signal.removeEventListener("abort", abort); resolve(); }, duration);
    signal.addEventListener("abort", abort, { once: true });
  });
}

/** 为内存示例提供真实应用接口；生产生命周期仍负责全部状态和迟到回写守卫。 */
function createPreviewApplications(scenario: ReviewStructureScenario) {
  let version = 1;
  const pending = new Set<() => void>();
  /** 每个示例 Snapshot 拥有独立身份及固定源码版本。 */
  const fixtureFor = (snapshotId: string) => {
    const fixture = createStructurePeekPreview(scenario as StructurePeekScenario);
    fixture.result.snapshotId = snapshotId;
    for (const content of fixture.contents) {
      content.snapshotId = snapshotId;
      for (const side of ["before", "after"] as const) if (content[side].state === "available") content[side].size = new TextEncoder().encode(content[side].text).length;
      if (content.source.after.state === "present") content.source.after.revision = `${snapshotId}-head`;
      if (content.after.state === "available") content.after.revision = `${snapshotId}-head`;
      fixture.result.sources = fixture.result.sources.map((source) => source.id === content.sourceId ? content.source : source);
    }
    if (scenario === "moved") {
      const destination = fixture.contents[1];
      const node = fixture.result.nodes.find((item) => item.id === "decl:welcome");
      fixture.result.nodes.push({ kind: "file", id: "file:destination", sourceId: destination.sourceId, containedChanges: { before: 0, after: 1, union: 1 } });
      if (node?.kind === "declaration" && node.after.state === "present") { node.after.value.parentId = "file:destination"; node.displayParentId = "file:destination"; }
    }
    return fixture;
  };
  /** 独立 Markdown File Change 用于证明 Peek 不改变普通文件选择。 */
  const snapshot = (): ReviewSnapshot => {
    const snapshotId = `integrated-snapshot-${version}`;
    const fixture = fixtureFor(snapshotId);
    return {
      reviewId: fixture.result.reviewId, snapshotId,
      baseRef: "refs/heads/main", baseRevision: "preview-base", comparisonBase: "preview-before",
      headRef: "refs/heads/structure-example", headRevision: `${snapshotId}-head`,
      fileChanges: [{ path: "notes.md", oldPath: null, status: "modified" }, ...fixture.contents.flatMap((content) => content.source.classification.kind === "file-change" ? [content.source.classification.fileChange] : [])],
    };
  };
  const review: ReviewApplicationAdapter = {
    /** 恢复当前内存示例 Snapshot。 */
    async switchReview() { return snapshot(); },
    /** 始终提供下一版示例，便于重复验收显式刷新。 */
    async checkReviewUpdate(current) { return { currentBaseRevision: current.baseRevision, currentHeadRevision: current.headRevision, availableBaseRevision: current.baseRevision, availableHeadRevision: `integrated-snapshot-${version + 1}-head` }; },
    /** 满足公共端口的无变化分支查询。 */
    async checkBranchSelectionUpdate() { return { changed: false, currentBaseRevision: "preview-base", currentHeadRevision: "preview-head", latestBaseRevision: "preview-base", latestHeadRevision: "preview-head" }; },
    /** 激活新示例版本，保留真实生命周期的重置逻辑。 */
    async refreshReview() { version += 1; return { kind: "refreshed", snapshot: snapshot() }; },
    /** 内存样本无需释放外部资源。 */
    async closeReview() { return { kind: "closed" }; },
    /** 读取独立 File Change 样本，缺对象时直接失败。 */
    async readFileChange(identity, path, signal) {
      await previewDelay(signal, 80);
      const content = fixtureFor(identity.snapshotId).contents.find((item) => item.source.classification.kind === "file-change" && item.source.classification.fileChange.path === path);
      if (!content || content.source.classification.kind !== "file-change") return { path: "notes.md", oldPath: null, status: "modified", contentKind: "text", beforeSize: 11, afterSize: 11, before: "# Old notes", after: "# New notes" };
      if (content.before.state === "unavailable" || content.after.state === "unavailable") throw new Error("Preview Git object cannot be read.");
      return {
        ...content.source.classification.fileChange, contentKind: "text",
        before: content.before.state === "available" ? content.before.text : null,
        after: content.after.state === "available" ? content.after.text : null,
        beforeSize: content.before.state === "available" ? content.before.size : null,
        afterSize: content.after.state === "available" ? content.after.size : null,
      } satisfies FileChangeContent;
    },
  };
  const structure: StructureApplicationAdapter = {
    /** 发送代表性进度，loading 场景等待验收者手动完成。 */
    async analyze(identity, onEvent, signal) {
      onEvent({ kind: "progress", progress: { phase: "extraction", side: "after", processedFiles: 1, totalFiles: 2 } });
      if (scenario === "loading") await new Promise<void>((resolve, reject) => {
        if (signal.aborted) { reject(new DOMException("Cancelled", "AbortError")); return; }
        const finish = () => { pending.delete(finish); signal.removeEventListener("abort", abort); resolve(); };
        const abort = () => { pending.delete(finish); signal.removeEventListener("abort", abort); reject(new DOMException("Cancelled", "AbortError")); };
        pending.add(finish); signal.addEventListener("abort", abort, { once: true });
      });
      else await previewDelay(signal, 160);
      const result = fixtureFor(identity.snapshotId).result;
      onEvent({ kind: "checkpoint", result });
      return result;
    },
    /** 只按固定 Snapshot 中的真实来源侧读取完整文件对。 */
    async readSource(identity, request, signal): Promise<StructurePeekContent> {
      await previewDelay(signal, 140);
      const content = fixtureFor(identity.snapshotId).contents.find((item) => {
        const side = item.source[request.side];
        return side.state === "present" && side.path === request.path;
      });
      if (!content) throw new Error("Preview source is unavailable.");
      return content;
    },
  };
  return { review, structure, initial: snapshot(), intent: fixtureFor(snapshot().snapshotId).intent, finishAnalysis: () => [...pending].forEach((finish) => finish()) };
}

/** 使用真实 ReviewReader 和 useReviewLifecycle 验证刷新、文件独立性、焦点及面板尺寸。 */
export function ReviewStructurePreview({ scenario = "changed" }: { scenario?: ReviewStructureScenario }) {
  const applications = useMemo(() => createPreviewApplications(scenario), [scenario]);
  const lifecycle = useReviewLifecycle(applications.review, { initialDiffMode: "side-by-side" }, undefined, applications.structure);
  const open = lifecycle.open;
  const [narrow, setNarrow] = useState(window.innerWidth < 1200);
  const openedPeek = useRef(false);
  const enteredStructure = useRef(false);
  /** 显式 viewport 变化使用与应用一致的窄窗口阈值，卸载时清理监听。 */
  useEffect(() => {
    const resize = () => setNarrow(window.innerWidth < 1200);
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);
  /** 仅打开样本 Review，后续第三视图和 Peek 仍经过真实用户意图接口。 */
  useEffect(() => {
    void open({ kind: "saved-review", reviewId: applications.initial.reviewId });
  }, [applications, open]);
  const reader = lifecycle.reader;
  /** 初次样本自动进入第三视图；刷新后保留真实生命周期的清空状态供检查。 */
  useEffect(() => {
    if (!reader || openedPeek.current) return;
    if (!enteredStructure.current) { enteredStructure.current = true; reader.setView("code-structure"); return; }
    if (reader.structure?.analysis.status === "completed") {
      openedPeek.current = true;
      reader.structure.navigate(applications.intent);
    }
  }, [applications.intent, reader]);
  const dark = document.documentElement.classList.contains("dark");
  return <div className="flex h-[calc(100vh-170px)] min-h-[480px] min-w-0 flex-col gap-2">
    <div className="flex shrink-0 flex-wrap items-center gap-2 rounded-lg border bg-muted/20 p-2">
      <span className="min-w-0 break-all text-xs text-muted-foreground">{reader?.snapshot.snapshotId} · File Changes: {reader?.fileChange.kind === "empty" ? "—" : reader?.fileChange.path}</span>
      <Button size="sm" variant="outline" onClick={() => reader?.structure?.navigate(applications.intent)}>重新定位示例</Button>
      <Button size="sm" variant="outline" disabled={!reader || !["available", "refresh-failed"].includes(reader.update.kind)} onClick={() => void reader?.refresh()}>刷新示例 Snapshot</Button>
      <Button size="sm" variant="outline" onClick={() => void reader?.checkForUpdate()}>检查示例更新</Button>
      {scenario === "loading" && <Button size="sm" variant="outline" onClick={applications.finishAnalysis}>完成示例分析</Button>}
    </div>
    <div className="min-h-0 flex-1">{reader && <ReviewReader review={reader} appearance={{ narrow, editorTheme: dark ? "vs-dark" : "vs", onOpenExternal: () => {} }} />}</div>
  </div>;
}

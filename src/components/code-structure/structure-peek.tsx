import { useCallback, useEffect, useMemo, useRef, useLayoutEffect } from "react";
import { Editor, type OnMount } from "@monaco-editor/react";
import { useTranslation } from "react-i18next";
import { BabysitDiffEditor } from "@/components/babysit-diff-editor";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { StructurePeekState } from "@/review/structure-peek";
import type { StructureGraphNavigation } from "@/review/structure-graph-types";
import type { StructureLocation, StructurePeekIdentity, StructureSide } from "@/review/structure-types";
import type { ReviewDiffMode } from "@/review/types";
import { reviewEditorIdentityKey, type ReviewEditorReveal } from "@/review/editor-source-reveal";
import { registerReviewEditorThemes, REVIEW_DARK_THEME, REVIEW_LIGHT_THEME } from "@/review/editor-theme";

/** Peek 的位置、异步读取与视图状态由 Review runtime 持有。 */
export type StructurePeekProps = {
  state: StructurePeekState | null;
  narrow: boolean;
  diffMode: ReviewDiffMode;
  editorTheme: string;
  viewState: unknown;
  onSaveViewState(identity: StructurePeekIdentity, state: unknown): void;
  onSelectLocation(index: number): void;
  onNavigate(intent: StructureGraphNavigation): void;
  onRetry(): void;
  onClose(reason: "keyboard" | "pointer"): void;
  onOpenFileChange(path: string): void;
};

/** 独立的 opaque 状态区分完整 Diff 与不可比较时的单侧阅读。 */
type PeekEditorState = { kind: "diff"; value: unknown; revealedRequestId?: number } | { kind: "single"; side: StructureSide; value: unknown; revealedRequestId?: number };

/** 只选择 Monaco 高亮语言，不参与源码、声明或 File Change 分类。 */
function sourceLanguage(path: string): string {
  const extension = path.split(".").pop() ?? "";
  return ({ rs: "rust", ts: "typescript", tsx: "typescript", js: "javascript", jsx: "javascript", json: "json", md: "markdown", css: "css", html: "html" } as Record<string, string>)[extension] ?? "plaintext";
}

/** 对不可比较的可读侧仍展示完整 Monaco 源文件，明确保留真实行号。 */
function ReadableSource({ text, path, side, location, identity, state, onSave, theme, requestId }: {
  text: string; path: string; side: StructureSide; location: StructureLocation; identity: StructurePeekIdentity;
  requestId: number; state: PeekEditorState | null; onSave: StructurePeekProps["onSaveViewState"]; theme: string;
}) {
  const editorRef = useRef<Parameters<OnMount>[0]>();
  const applied = useRef<number | undefined>(state?.kind === "single" && state.side === side ? state.revealedRequestId : undefined);
  const onMount = useCallback<OnMount>((editor, monaco) => {
    editorRef.current = editor;
    if (state?.kind === "single" && state.side === side) editor.restoreViewState(state.value as NonNullable<ReturnType<typeof editor.saveViewState>>);
    if (location.side !== side || applied.current === requestId) return;
    const range = editor.getModel()?.validateRange(new monaco.Range(location.range.startLine, location.range.startColumn, location.range.endLine, location.range.endColumn));
    if (range) { editor.setSelection(range); editor.revealRangeInCenter(range, monaco.editor.ScrollType.Immediate); applied.current = requestId; }
  }, [location, requestId, side, state]);
  useLayoutEffect(() => () => {
    const value = editorRef.current?.saveViewState();
    if (value) onSave(identity, { kind: "single", side, value, revealedRequestId: applied.current });
  }, [identity, onSave, side]);
  return <Editor height="100%" value={text} language={sourceLanguage(path)} theme={theme === "light" ? REVIEW_LIGHT_THEME : theme === "vs-dark" ? REVIEW_DARK_THEME : theme} beforeMount={registerReviewEditorThemes} onMount={onMount} options={{ readOnly: true, wordWrap: "off", automaticLayout: true, fontFamily: "'JetBrains Mono Variable', monospace" }} />;
}

/** 受控完整文件 Peek；只发出导航动作，不改变 File Changes 选择或图相机。 */
export function StructurePeek({ state, narrow, diffMode, editorTheme, viewState, onSaveViewState, onSelectLocation, onNavigate, onRetry, onClose, onOpenFileChange }: StructurePeekProps) {
  const { t } = useTranslation("structure");
  const cached = viewState && typeof viewState === "object" && "kind" in viewState ? viewState as PeekEditorState : null;
  const revealedRequests = useRef(new Map<string, number>());
  const selection = state?.selection;
  const location = selection?.locations[state?.locationIndex ?? 0];
  const source = selection?.sources.find((item) => item.id === location?.sourceId);
  const reviewId = selection?.reviewId;
  const snapshotId = selection?.snapshotId;
  const sourceId = source?.id;
  const requestId = state?.requestId;
  const identity = useMemo(() => reviewId !== undefined && snapshotId !== undefined && sourceId !== undefined ? { reviewId, snapshotId, sourceId } : null, [reviewId, snapshotId, sourceId]);
  const restoredRequestId = cached?.kind === "diff" ? cached.revealedRequestId : undefined;
  const reveal = useMemo(() => identity && location && requestId !== undefined ? { identity, requestId, side: location.side, range: location.range, preserveScroll: restoredRequestId === requestId } : undefined, [identity, location, requestId, restoredRequestId]);
  // 相同请求重新挂载时保留手动滚动；只有新的显式导航序号才覆盖 viewState。
  useEffect(() => { if (identity && restoredRequestId !== undefined) revealedRequests.current.set(reviewEditorIdentityKey(identity), restoredRequestId); }, [identity, restoredRequestId]);
  const onRevealApplied = useCallback((request: ReviewEditorReveal) => { revealedRequests.current.set(reviewEditorIdentityKey(request.identity), request.requestId); }, []);
  const saveDiff = useCallback((id: StructurePeekIdentity, value: unknown) => onSaveViewState(id, { kind: "diff", value, revealedRequestId: revealedRequests.current.get(reviewEditorIdentityKey(id)) }), [onSaveViewState]);
  if (!state || !selection) return null;
  const ready = state.load.status === "ready" ? state.load.content : null;
  const content = ready && identity && reviewEditorIdentityKey(ready) === reviewEditorIdentityKey(identity) ? ready : null;
  const unavailable = content && (["before", "after"] as const).filter((side) => content[side].state === "unavailable");
  const readableSide = content?.after.state === "available" ? "after" : content?.before.state === "available" ? "before" : null;
  const sourcePath = source?.after.state === "present" ? source.after.path : source?.before.state === "present" ? source.before.path : "";
  return <section className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-background" aria-label={t("peek.title")} onKeyDown={(event) => {
    // 来源下拉框先消费 Escape；Portal 的 React 冒泡不能顺带关闭整个 Peek。
    if (event.target instanceof Element && event.target.closest('[role="listbox"], [role="combobox"][aria-expanded="true"]')) return;
    if (event.key === "Escape" && !event.defaultPrevented) { event.preventDefault(); event.stopPropagation(); onClose("keyboard"); }
  }}>
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-b p-2">
      <strong className="min-w-0 flex-1 truncate text-sm" title={selection.title}>{selection.title || t("peek.title")}</strong>
      {source?.classification.kind === "unchanged" && <Badge variant="secondary">{t("peek.unchanged")}</Badge>}
      <Button size="sm" variant="ghost" onClick={(event) => onClose(event.detail === 0 ? "keyboard" : "pointer")}>{t("peek.close")}</Button>
      {location && <Select value={String(state.locationIndex)} onValueChange={(value) => { if (value !== null) onSelectLocation(Number(value)); }}>
        <SelectTrigger className="min-w-0 max-w-full basis-full text-xs" aria-label={t("peek.sources")}><SelectValue className="min-w-0 truncate">{(() => {
          const side = source?.[location.side];
          const path = side?.state === "present" ? side.path : sourcePath;
          return `${path.split("/").pop()}:${location.range.startLine} · ${t(`peek.${location.side}`)} · ${t(`peek.${location.role}`)}`;
        })()}</SelectValue></SelectTrigger>
        <SelectContent className="min-w-0 max-w-[calc(100vw-24px)]" alignItemWithTrigger={false}>
          {selection.locations.map((item, index) => {
            const side = selection.sources.find((candidate) => candidate.id === item.sourceId)?.[item.side];
            const path = side?.state === "present" ? side.path : "";
            // ItemText 默认不收缩；局部覆盖后让完整路径折行，并把可辨识的文件名放在首行。
            return <SelectItem key={index} value={String(index)} className="min-w-0 text-xs [&>div]:min-w-0 [&>div]:shrink [&>div]:whitespace-normal">
              <span className="grid min-w-0 flex-1 gap-1 [overflow-wrap:anywhere]">
                <span className="font-medium">{path.split("/").pop()}:{item.range.startLine} · {t(`peek.${item.side}`)} · {t(`peek.${item.role}`)}</span>
                <span className="text-muted-foreground">{path}</span>
              </span>
            </SelectItem>;
          })}
        </SelectContent>
      </Select>}
      {source && <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground" title={sourcePath}>{t("peek.fullFile")} · {sourcePath}</span>}
      {selection.targetDefinition && <Button size="sm" variant="outline" onClick={() => onNavigate(selection.targetDefinition!)}>{t("peek.target")}</Button>}
      {source?.classification.kind === "file-change" && <Button size="sm" variant="outline" onClick={() => onOpenFileChange(source.classification.kind === "file-change" ? source.classification.fileChange.path : "")}>{t("peek.openFileChange")}</Button>}
    </div>
    {selection.unavailableReason && <p role="status" className="p-4 text-sm text-muted-foreground">{t(selection.unavailableReason === "external-source-not-indexed" ? "peek.external" : "peek.noLocation")}</p>}
    {!selection.unavailableReason && !content && state.load.status !== "failed" && <p role="status" className="p-4 text-sm">{t("peek.loading")}</p>}
    {state.load.status === "failed" && <div role="alert" className="break-words p-4 text-sm [overflow-wrap:anywhere]"><p>{state.load.message}</p>{state.load.retryable && <Button onClick={onRetry}>{t("peek.retry")}</Button>}</div>}
    {content && identity && location && <>
      {(["before", "after"] as const).map((side) => {
        const value = content[side];
        return value.state === "unavailable" ? <div role="status" key={side} className="shrink-0 break-words border-b px-3 py-2 text-xs [overflow-wrap:anywhere]"><p>{t("peek.unavailable", { side: t(`peek.${side}`), message: value.message })}</p>{value.retryable && <Button size="sm" onClick={onRetry}>{t("peek.retry")}</Button>}</div>
          : value.state === "absent" ? <p key={side} className="shrink-0 px-3 py-1 text-xs text-muted-foreground">{t("peek.absent", { side: t(`peek.${side}`) })}</p> : null;
      })}
      {unavailable?.length ? <>{readableSide && <><p className="shrink-0 px-3 py-1 text-xs text-muted-foreground">{t("peek.readableOnly")}</p><div className="min-h-0 flex-1"><ReadableSource key={`${reviewEditorIdentityKey(identity)}:${readableSide}:${state.requestId}`} text={content[readableSide].state === "available" ? content[readableSide].text : ""} path={content[readableSide].state === "available" ? content[readableSide].path : sourcePath} side={readableSide} location={location} identity={identity} state={cached} requestId={state.requestId} onSave={onSaveViewState} theme={editorTheme} /></div></>}</>
        : <div className="min-h-0 flex-1"><BabysitDiffEditor key={reviewEditorIdentityKey(identity)} identity={identity} viewState={cached?.kind === "diff" ? cached.value : null} onSaveViewState={saveDiff} reveal={reveal} onRevealApplied={onRevealApplied} theme={editorTheme} height="100%" language={sourceLanguage(sourcePath)} original={content.before.state === "available" ? content.before.text : ""} modified={content.after.state === "available" ? content.after.text : ""} renderSideBySide={!narrow && diffMode === "side-by-side"} /></div>}
    </>}
  </section>;
}

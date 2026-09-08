import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { CodeStructureView } from "@/components/code-structure/code-structure-view";
import { StructurePeek } from "@/components/code-structure/structure-peek";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import type { StructureGraphNavigation } from "@/review/structure-graph-types";
import type { StructureReaderBinding } from "@/review/structure-runtime";
import type { ReviewDiffMode } from "@/review/types";

/** 将同一 Review 的受控图和完整源码 Peek 组合为可调整的上下区域。 */
export function ReviewStructureView({ structure, narrow, diffMode, editorTheme, onOpenFileChange }: {
  structure: StructureReaderBinding;
  narrow: boolean;
  diffMode: ReviewDiffMode;
  editorTheme: string;
  onOpenFileChange(path: string): void;
}) {
  const { t } = useTranslation("structure");
  const graphRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLElement | null>(null);
  /** 记录真实图焦点，以便键盘关闭 Peek 后返回原交互位置。 */
  const navigate = (intent: StructureGraphNavigation) => {
    const active = document.activeElement;
    if (active instanceof HTMLElement && graphRef.current?.contains(active)) triggerRef.current = active;
    structure.navigate(intent);
  };
  /** 关闭只改变 Peek；图相机、选择和折叠均留在 Review runtime。 */
  const closePeek = (reason: "keyboard" | "pointer") => {
    structure.closePeek();
    if (reason === "keyboard") requestAnimationFrame(() => {
      const target = triggerRef.current;
      if (target?.isConnected && graphRef.current?.contains(target)) target.focus();
      else graphRef.current?.focus();
    });
  };
  return <ResizablePanelGroup orientation="vertical" className="min-h-0 flex-1"
    defaultLayout={{ "structure-graph": structure.peek ? 100 - structure.peekSize : 100, ...(structure.peek ? { "structure-peek": structure.peekSize } : {}) }}
    onLayoutChanged={(layout, meta) => { if (meta.isUserInteraction && layout["structure-peek"] !== undefined) structure.setPeekSize(layout["structure-peek"]); }}>
    <ResizablePanel id="structure-graph" minSize="25%" defaultSize={`${structure.peek ? 100 - structure.peekSize : 100}%`}>
      <div ref={graphRef} tabIndex={-1} aria-label={t("title")} className="h-full min-h-0 outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <CodeStructureView analysis={structure.analysis} presentation={structure.presentation} onPresentationChange={structure.setPresentation}
          onNavigate={navigate} onCancel={structure.cancel} onRetry={structure.retry} colorMode={editorTheme === "vs-dark" ? "dark" : "light"} />
      </div>
    </ResizablePanel>
    {structure.peek && <ResizableHandle withHandle aria-label={t("resizePeek")} />}
    {structure.peek && <ResizablePanel id="structure-peek" minSize="20%" maxSize="75%" defaultSize={`${structure.peekSize}%`}>
      <StructurePeek state={structure.peek} narrow={narrow} diffMode={diffMode} editorTheme={editorTheme}
        viewState={structure.peekViewState} onSaveViewState={structure.savePeekViewState} onSelectLocation={structure.selectLocation}
        onNavigate={navigate} onRetry={structure.retrySource} onClose={closePeek} onOpenFileChange={onOpenFileChange} />
    </ResizablePanel>}
  </ResizablePanelGroup>;
}

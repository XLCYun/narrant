import { createStructureMultiSourceFixture } from "./structure-multi-source-fixture";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { StructurePeek } from "@/components/code-structure/structure-peek";
import { Button } from "@/components/ui/button";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { resolveStructurePeekNavigation, type StructurePeekState } from "@/review/structure-peek";
import type { StructureGraphNavigation } from "@/review/structure-graph-types";
import type { StructureLocation, StructurePeekContent, StructurePeekIdentity, StructureResult } from "@/review/structure-types";
import { structureContractPreview, structurePeekPreview } from "./structure-fixture";

/** 只在通用 Harness 内构造完整源码，不触发后端读取或修改真实数据。 */
export type StructurePeekScenario = "changed" | "deleted" | "moved" | "renamed" | "unchanged" | "unavailable" | "loading" | "error" | "external" | "relationship" | "long" | "file-added" | "file-deleted" | "empty-file" | "long-path" | "multiple-impls" | "multiple-occurrences";

/** 构造行号明确的 CRLF/Unicode 样本；移动始终使用两个独立真实文件对。 */
export function createStructurePeekPreview(scenario: StructurePeekScenario): { result: StructureResult; contents: StructurePeekContent[]; intent: StructureGraphNavigation } {
  if (scenario === "long-path" || scenario === "multiple-impls" || scenario === "multiple-occurrences") return createStructureMultiSourceFixture(scenario);
  const result = structuredClone(structureContractPreview);
  const content = structuredClone(structurePeekPreview);
  let intent: StructureGraphNavigation = { kind: "node", nodeId: "decl:welcome" };
  const contents = [content];
  const node = result.nodes.find((item) => item.id === "decl:welcome")!;
  if (node.kind !== "declaration" || node.after.state !== "present" || node.before.state !== "present") throw new Error("Preview declaration missing");
  if (scenario === "long" || scenario === "deleted" || scenario === "moved") {
    const prefix = Array.from({ length: 119 }, (_, index) => `// 完整文件上下文 ${index + 1}`).join("\r\n") + "\r\n";
    const suffix = "\r\n" + Array.from({ length: 160 }, (_, index) => `// 后续上下文 ${index + 1}`).join("\r\n");
    if (content.before.state === "available") content.before.text = prefix + 'export function welcome() {\r\n  console.log("删除🙂");\r\n}' + suffix;
    if (content.after.state === "available") content.after.text = prefix + 'export function welcome() {\r\n  console.log("新代码🙂");\r\n}' + suffix;
    node.before.value.locations = [{ sourceId: content.sourceId, side: "before", role: "declaration", range: { startLine: 121, startColumn: 3, endLine: 121, endColumn: 26 } }];
    node.after.value.locations = [{ ...node.before.value.locations[0], side: "after" }];
    if (scenario === "deleted") {
      node.after = { state: "absent" }; node.changes = ["deleted"];
      if (content.after.state === "available") content.after.text = prefix + suffix;
    }
  }
  if (scenario === "moved") {
    const destination = structuredClone(content);
    destination.sourceId = "preview:destination.ts";
    destination.source.id = destination.sourceId;
    destination.source.classification = { kind: "file-change", fileChange: { path: "destination.ts", oldPath: null, status: "modified" } };
    for (const side of ["before", "after"] as const) {
      if (destination.source[side].state === "present") destination.source[side].path = "destination.ts";
      if (destination[side].state === "available") destination[side].path = "destination.ts";
    }
    if (destination.before.state === "available") destination.before.text = "// Destination previously contained no welcome function.\n";
    if (content.after.state === "available") content.after.text = "// The function moved away from this file.\n";
    if (node.after.state === "present") node.after.value.locations[0].sourceId = destination.sourceId;
    node.changes = ["moved"];
    contents.push(destination); result.sources.push(destination.source);
  }
  if (scenario === "renamed") {
    content.source.classification = { kind: "file-change", fileChange: { path: "renamed/welcome.ts", oldPath: "welcome.ts", status: "renamed" } };
    if (content.source.after.state === "present") content.source.after.path = "renamed/welcome.ts";
    if (content.after.state === "available") content.after.path = "renamed/welcome.ts";
  }
  if (scenario === "unchanged") {
    content.source.classification = { kind: "unchanged", path: "welcome.ts" };
    if (content.before.state === "available" && content.after.state === "available") content.after.text = content.before.text;
  }
  if (scenario === "unavailable") content.before = { state: "unavailable", reason: "git-object-unavailable", message: "The before Git object cannot be read.", retryable: true };
  if (scenario === "external") intent = { kind: "node", nodeId: "external:sonner:toast" };
  if (scenario === "relationship") intent = { kind: "relationship", relationshipId: "relation:welcome:toast:calls" };
  if (scenario === "file-added" || scenario === "file-deleted" || scenario === "empty-file") {
    // 文件入口不伪造空文件中的声明；缺侧描述符与读取结果一起保持真实 Git 语义。
    result.nodes = result.nodes.filter((item) => item.kind === "file").map((item) => ({ ...item, containedChanges: { before: 0, after: 0, union: 0 } }));
    result.implementationGroups = [];
    result.relationships = [];
    result.unresolvedRelationships = [];
    result.initialFocus = { nodeIds: ["file:welcome"], relationshipIds: [] };
    result.summary = { changedDeclarations: 0, relationshipChanges: 0, incompleteDeclarations: 0, unresolvedOccurrences: 0, complete: true };
    intent = { kind: "node", nodeId: "file:welcome" };
    if (scenario === "file-added") {
      content.source.before = { state: "absent" };
      content.before = { state: "absent", reason: "git-tree-absence" };
      content.source.classification = { kind: "file-change", fileChange: { path: "welcome.ts", oldPath: null, status: "added" } };
    } else if (scenario === "file-deleted") {
      content.source.after = { state: "absent" };
      content.after = { state: "absent", reason: "git-tree-absence" };
      content.source.classification = { kind: "file-change", fileChange: { path: "welcome.ts", oldPath: null, status: "deleted" } };
      if (content.before.state === "available") {
        content.before.text += '\nexport const previousContext = "完整旧文件🙂";\n';
        content.before.size = new TextEncoder().encode(content.before.text).length;
      }
    } else if (content.before.state === "available") {
      // 真实存在的零字节 blob 保留 available 和完整身份，不能改成 absent。
      content.before.text = "";
      content.before.size = 0;
    }
    result.coverage.files = result.coverage.files.filter((file) => content.source[file.side].state === "present")
      .map((file) => ({ ...file, extraction: "complete", relationships: "complete", diagnostics: [] }));
  }
  result.sources[0] = content.source;
  return { result, contents, intent };
}

/** 可交互验收独立 Peek、来源菜单、重复定位、关闭焦点和底部分割条。 */
export function StructurePeekPreview({ scenario = "changed" }: { scenario?: StructurePeekScenario }) {
  const fixture = useMemo(() => createStructurePeekPreview(scenario), [scenario]);
  const requestGeneration = useRef(0);
  /** 把手工内容附到受控选择，不模拟生产后端。 */
  const open = useCallback((intent: StructureGraphNavigation): StructurePeekState | null => {
    const selection = resolveStructurePeekNavigation(fixture.result, intent);
    if (!selection) return null;
    const content = fixture.contents.find((item) => item.sourceId === selection.locations[0]?.sourceId);
    return { selection, locationIndex: 0, requestId: ++requestGeneration.current, load: scenario === "loading" ? { status: "loading" } : scenario === "error" ? { status: "failed", message: "Preview source read failed.", retryable: true } : content ? { status: "ready", content } : { status: "idle" } };
  }, [fixture, scenario]);
  const [state, setState] = useState(() => open(fixture.intent));
  const [narrow, setNarrow] = useState(window.innerWidth < 1200);
  const [lastAction, setLastAction] = useState("");
  const trigger = useRef<HTMLButtonElement>(null);
  const cache = useRef<Record<string, unknown>>({});
  const save = useCallback((identity: StructurePeekIdentity, value: unknown) => { cache.current[identity.sourceId] = value; }, []);
  /** 显式浏览器 viewport 变化直接驱动窄窗口布局，并随 Harness 卸载清理监听。 */
  useEffect(() => {
    const resize = () => setNarrow(window.innerWidth < 1200);
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);
  /** 保留受控选择，切换到所选位置对应的真实文件内容。 */
  const navigateLocation = (index: number) => setState((current) => {
    if (!current) return current;
    const location: StructureLocation | undefined = current.selection.locations[index];
    const content = fixture.contents.find((item) => item.sourceId === location?.sourceId);
    return location && content ? { ...current, locationIndex: index, requestId: ++requestGeneration.current, load: { status: "ready", content } } : current;
  });
  return <div className="h-[calc(100vh-120px)] min-h-96 w-full">
    <ResizablePanelGroup orientation="vertical">
      <ResizablePanel defaultSize="25%" minSize="15%"><div className="flex h-full flex-wrap items-center justify-center gap-3 bg-muted/30 p-4">
        <Button ref={trigger} onClick={() => setState(open(fixture.intent))}>Open source</Button>
        <Button variant="outline" onClick={() => navigateLocation(state?.locationIndex ?? 0)}>Reveal again</Button>
        <span role="status" className="text-xs">{lastAction}</span>
      </div></ResizablePanel>
      {state && <><ResizableHandle withHandle aria-label="Resize source Peek" /><ResizablePanel defaultSize="75%" minSize="35%">
        <StructurePeek state={state} narrow={narrow} diffMode="side-by-side" editorTheme="light" viewState={cache.current[state.selection.locations[state.locationIndex]?.sourceId]} onSaveViewState={save} onSelectLocation={navigateLocation} onNavigate={(intent) => setState(open(intent))} onRetry={() => setState(open(fixture.intent))} onClose={(reason) => { setState(null); setLastAction(`Closed by ${reason}`); if (reason === "keyboard") trigger.current?.focus(); }} onOpenFileChange={(path) => setLastAction(`File Changes: ${path}`)} />
      </ResizablePanel></>}
    </ResizablePanelGroup>
  </div>;
}

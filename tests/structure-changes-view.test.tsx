import type { ComponentType, ReactNode } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/lib/i18n";
import StructureChangesView from "@/components/structure-review/structure-changes-view";
import { structureClassDiagramFixture, structureFieldTypesFixture, structureFileModulesFixture, structureHiddenReferencesFixture, structureMixedLanguageFixture, structureMixedLanguagesFixture, structureReviewFixture, structureTestCodeFixture } from "@/dev/structure-review-fixture";
import type { ObjectData, ObjectNode, StructureEdge, StructureLayout } from "@/structure-review/graph-presentation";
import * as graphPresentation from "@/structure-review/graph-presentation";
import type { StructureBinding, StructureComparison } from "@/structure-review/types";
import type { DiffEditorProps } from "@monaco-editor/react";

// 编辑器本身在浏览器验收；组件测试验证送入 Monaco 的两侧原文和展示选项。
vi.mock("@/infrastructure/local-monaco", () => ({ configureLocalMonaco: vi.fn() }));
vi.mock("@monaco-editor/react", () => ({
  DiffEditor: ({ original, modified, originalLanguage, modifiedLanguage, theme, options }: DiffEditorProps) => <div
    data-testid="source-diff" data-theme={theme} data-original-language={originalLanguage} data-modified-language={modifiedLanguage}
    data-inline={!options?.renderSideBySide} data-read-only={options?.readOnly} data-ignore-whitespace={options?.ignoreTrimWhitespace}
  ><pre data-testid="original-source">{original}</pre><pre data-testid="modified-source">{modified}</pre></div>,
}));

// jsdom 没有画布尺寸；保留真实布局和节点交互，只替换 React Flow 的浏览器绘制层。
vi.mock("@xyflow/react", async (importOriginal) => ({
  ...await importOriginal<typeof import("@xyflow/react")>(),
  ReactFlow: ({ nodes, edges, nodeTypes, children, onPaneClick }: { nodes: ObjectNode[]; edges: StructureEdge[]; nodeTypes: { codeObject: ComponentType<{ id: string; data: ObjectData }> }; children: ReactNode; onPaneClick(): void }) => {
    const ports = new Map(nodes.flatMap((node) => node.data.ports.map((port) => [port.id, port.objectId] as const)));
    return <div data-testid="rendered-graph" data-layout={JSON.stringify(nodes.map((node) => ({ id: node.id, position: node.position, style: node.style })))}>{nodes.map((node) => <nodeTypes.codeObject key={node.id} id={node.id} data={node.data} />)}{children}
      {edges.map((edge) => <div key={edge.id} data-testid="visible-relation" title={edge.data?.collapsedSource ?? edge.data?.collapsedTarget}>{ports.get(edge.sourceHandle!)} → {ports.get(edge.targetHandle!)} · {edge.label}</div>)}
      <button onClick={onPaneClick}>Graph background</button>
    </div>;
  },
  Handle: () => null,
  Background: () => null,
  Controls: () => null,
  useUpdateNodeInternals: () => () => {},
}));

/** 用完整的比较结果驱动公开结构视图接口。 */
function binding(comparison = structureReviewFixture()): StructureBinding {
  return { state: { kind: "ready", comparison }, options: { includeTests: false }, setOptions: vi.fn(), retry: vi.fn(), cancel: vi.fn() };
}

/** 勾选显示项后用 Escape 收起菜单，模拟继续操作画布。 */
async function toggleRearrangement() {
  if (!screen.queryByRole("menuitemcheckbox", { name: "折叠时重新布局" })) {
    fireEvent.click(screen.getByRole("button", { name: "显示选项" }));
  }
  const option = await screen.findByRole("menuitemcheckbox", { name: "折叠时重新布局" });
  fireEvent.click(option);
  fireEvent.keyDown(option, { key: "Escape" });
}

beforeEach(async () => { await i18n.changeLanguage("zh-CN"); });
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it("switches graph languages locally, clears old details and keeps cross-language relationships hidden", async () => {
  const current = binding(structureMixedLanguagesFixture());
  render(<StructureChangesView binding={current} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  expect(screen.getByRole("combobox", { name: "结构图语言" })).toHaveTextContent("Rust");
  expect(within(graph).getByRole("button", { name: "PullRequestGroup", exact: true })).toBeInTheDocument();
  expect(within(graph).queryByRole("button", { name: "PullRequestViewModel", exact: true })).not.toBeInTheDocument();
  fireEvent.click(within(graph).getByRole("button", { name: /pub pull_requests:.*自身修改/ }));
  expect(screen.getAllByTestId("visible-relation")).toHaveLength(1);
  fireEvent.click(screen.getByRole("combobox", { name: "结构图语言" }));
  const typescriptOption = await screen.findByRole("option", { name: "TypeScript" });
  fireEvent.pointerDown(typescriptOption);
  fireEvent.pointerUp(typescriptOption);
  fireEvent.click(typescriptOption);
  await screen.findByRole("button", { name: "PullRequestViewModel", exact: true });
  const next = screen.getByTestId("rendered-graph");
  expect(within(next).queryByRole("button", { name: "PullRequestGroup", exact: true })).not.toBeInTheDocument();
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
  expect(screen.queryByTestId("source-diff")).not.toBeInTheDocument();
  fireEvent.click(within(next).getByRole("button", { name: /export async function refreshPullRequests/ }));
  expect(screen.getAllByTestId("visible-relation").map((edge) => edge.textContent)).toEqual(["ts-refresh → ts-persist · + code.calls"]);
  const sidebar = screen.getByRole("complementary", { name: "结构详情" });
  expect(sidebar).toHaveTextContent("TypeScript".toLowerCase());
  expect(sidebar).not.toHaveTextContent("code.calls → refresh");
  fireEvent.click(screen.getByRole("button", { name: "显示选项" }));
  fireEvent.click(await screen.findByRole("menuitemcheckbox", { name: "显示全部对象" }));
  await waitFor(() => expect(screen.queryByText("正在排列结构图")).not.toBeInTheDocument());
  expect(screen.queryByRole("button", { name: "PullRequestGroup", exact: true })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("combobox", { name: "结构图语言" }));
  const rustOption = await screen.findByRole("option", { name: "Rust" });
  fireEvent.pointerDown(rustOption);
  fireEvent.pointerUp(rustOption);
  fireEvent.click(rustOption);
  expect(await screen.findByRole("button", { name: "PullRequestGroup", exact: true })).toBeInTheDocument();
  expect(screen.queryByTestId("source-diff")).not.toBeInTheDocument();
  expect(current.retry).not.toHaveBeenCalled();
  expect(current.setOptions).not.toHaveBeenCalled();
});

it("keeps the language selector visible for single-language and empty results", async () => {
  const view = render(<StructureChangesView binding={binding()} dark={false} />);
  expect(screen.getByRole("combobox", { name: "结构图语言" })).toHaveTextContent("Rust");
  const comparison = structureReviewFixture();
  comparison.before = { root: [], nodes: [], externalSymbols: {} };
  comparison.after = { root: [], nodes: [], externalSymbols: {} };
  comparison.diff = { nodes: [] };
  view.rerender(<StructureChangesView binding={binding(comparison)} dark={false} />);
  expect(screen.getByRole("combobox", { name: "结构图语言" })).toBeDisabled();
  expect(screen.getByRole("combobox", { name: "结构图语言" })).toHaveTextContent("暂无语言");
  expect(await screen.findByText("没有结构变化")).toBeInTheDocument();
});

it("compares original source as one read-only inline diff and follows theme changes", async () => {
  const comparison = structureReviewFixture();
  const view = render(<StructureChangesView binding={binding(comparison)} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  fireEvent.click(within(graph).getByRole("button", { name: /pub async fn run_review.*自身修改/ }));
  expect(screen.getAllByTestId("source-diff")).toHaveLength(1);
  expect(screen.getByTestId("original-source").textContent).toBe(comparison.before.nodes.find((node) => node.id === "run")!.source);
  expect(screen.getByTestId("modified-source").textContent).toBe(comparison.after.nodes.find((node) => node.id === "run")!.source);
  expect(screen.getByTestId("modified-source").textContent).toContain("\n    // 固定本次审核使用的源码版本。\n");
  const editor = screen.getByTestId("source-diff");
  expect(editor).toHaveAttribute("data-inline", "true");
  expect(editor).toHaveAttribute("data-read-only", "true");
  expect(editor).toHaveAttribute("data-ignore-whitespace", "false");
  expect(editor).toHaveAttribute("data-original-language", "rust");
  expect(editor).toHaveAttribute("data-modified-language", "rust");
  expect(editor).toHaveAttribute("data-theme", "narrant-github-light");
  view.rerender(<StructureChangesView binding={binding(comparison)} dark />);
  expect(editor).toHaveAttribute("data-theme", "narrant-github-dark");
  expect(screen.queryByText("自身实现")).not.toBeInTheDocument();
});

it("uses an empty side only for added or removed objects and reports unavailable source", async () => {
  const comparison = structureReviewFixture();
  comparison.after.nodes.find((node) => node.id === "run")!.source = null;
  render(<StructureChangesView binding={binding(comparison)} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  fireEvent.click(within(graph).getByRole("button", { name: /fn prepare_snapshot.*新增/ }));
  expect(screen.getByTestId("original-source").textContent).toBe("");
  expect(screen.getByTestId("modified-source").textContent).toBe(comparison.after.nodes.find((node) => node.id === "new")!.source);
  fireEvent.click(within(graph).getByRole("button", { name: /fn load_legacy_state.*删除/ }));
  expect(screen.getByTestId("original-source").textContent).toBe(comparison.before.nodes.find((node) => node.id === "old")!.source);
  expect(screen.getByTestId("modified-source").textContent).toBe("");
  fireEvent.click(within(graph).getByRole("button", { name: /pub async fn run_review.*自身修改/ }));
  expect(screen.queryByTestId("source-diff")).not.toBeInTheDocument();
  expect(screen.getByText("当前对象没有可对比的原始源码。")).toBeInTheDocument();
  expect(screen.queryByText("该侧没有此对象")).not.toBeInTheDocument();
});

it("shows unchanged field type relations on selection and switches precise field endpoints", async () => {
  render(<StructureChangesView binding={binding(structureFieldTypesFixture())} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  expect(within(graph).getByRole("button", { name: "DiscoveredPullRequest", exact: true })).toBeInTheDocument();
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
  fireEvent.click(within(graph).getByRole("button", { name: /pub pull_requests:.*自身修改/ }));
  expect(screen.getAllByTestId("visible-relation").map((edge) => edge.textContent)).toEqual(["pull-requests → Item · code.type_use"]);
  fireEvent.click(within(graph).getByRole("button", { name: /pub repository:.*自身修改/ }));
  expect(screen.getAllByTestId("visible-relation").map((edge) => edge.textContent).sort()).toEqual(["repository → LegacyRepository · − code.type_use", "repository → Repository · + code.type_use"]);
  fireEvent.click(within(graph).getByRole("button", { name: /pub error:.*自身修改/ }));
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
  fireEvent.click(within(graph).getByRole("button", { name: /pub parent:.*新增/ }));
  expect(screen.getAllByTestId("visible-relation").map((edge) => edge.textContent)).toEqual(["parent → Group · + code.type_use"]);
  fireEvent.click(within(graph).getByRole("button", { name: "Graph background" }));
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
  expect(screen.getByTestId("rendered-graph")).toBe(graph);
});

it("offers a controlled analysis switch in ready, empty, loading, failed and cancelled states", async () => {
  const current = binding();
  const view = render(<StructureChangesView binding={current} dark={false} />);
  const toggle = screen.getByRole("switch", { name: "分析测试代码" });
  expect(toggle).not.toBeChecked();
  fireEvent.click(toggle);
  expect(current.setOptions).toHaveBeenCalledWith({ includeTests: true });
  for (const state of [{ kind: "loading" }, { kind: "cancelled" }, { kind: "failed", detail: "unavailable" }, { kind: "ready", comparison: structureHiddenReferencesFixture() }] as const) {
    view.rerender(<StructureChangesView binding={{ ...current, state, options: { includeTests: true } }} dark={false} />);
    const toggle = screen.getByRole("switch", { name: "分析测试代码" });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    expect(current.setOptions).toHaveBeenLastCalledWith({ includeTests: false });
  }
  await waitFor(() => expect(screen.getByText("没有可显示的结构变化")).toBeInTheDocument());
});

it("shows both test implementations when the analysis result includes a test-only change", async () => {
  const base = structureReviewFixture();
  const comparison = structureTestCodeFixture({ ...base, before: base.after, diff: { nodes: [] } }, true);
  render(<StructureChangesView binding={{ ...binding(comparison), options: { includeTests: true } }} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  fireEvent.click(within(graph).getByRole("button", { name: /checks_snapshot_identity.*自身修改/ }));
  const sidebar = screen.getByRole("complementary", { name: "结构详情" });
  expect(sidebar).toHaveTextContent("{ assert!(snapshot.is_ready()); }");
  expect(sidebar).toHaveTextContent("{ assert!(snapshot.has_matching_identity()); }");
});

it("keeps details beside the graph and switches exact objects without remounting the canvas", async () => {
  render(<StructureChangesView binding={binding()} dark={false} />);
  const sidebar = screen.getByRole("complementary", { name: "结构详情" });
  expect(within(sidebar).getByText("选择图中的对象，在此查看详情。")).toBeInTheDocument();
  expect(screen.getByRole("separator", { name: "调整结构详情宽度" })).toBeInTheDocument();
  const graph = await screen.findByTestId("rendered-graph");
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
  const added = within(graph).getByRole("button", { name: /fn prepare_snapshot.*新增/ });
  fireEvent.click(added);
  expect(within(sidebar).getByRole("heading", { name: "prepare_snapshot" })).toBeInTheDocument();
  expect(within(sidebar).getByText("该侧没有此对象")).toBeInTheDocument();
  expect(added).toHaveAttribute("aria-pressed", "true");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.getByTestId("rendered-graph")).toBe(graph);
  expect(screen.getAllByTestId("visible-relation").map((edge) => edge.textContent).sort()).toEqual(["new → save · + code.calls", "run → new · + code.calls"]);

  const removed = within(graph).getByRole("button", { name: /fn load_legacy_state.*删除/ });
  fireEvent.click(removed);
  expect(within(sidebar).getByRole("heading", { name: "load_legacy_state" })).toBeInTheDocument();
  expect(added).toHaveAttribute("aria-pressed", "false");
  expect(removed).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByTestId("rendered-graph")).toBe(graph);
  expect(screen.getAllByTestId("visible-relation").map((edge) => edge.textContent)).toEqual(["run → old · − code.calls"]);

  fireEvent.click(within(graph).getByRole("button", { name: "Review", exact: true }));
  expect(within(sidebar).getByRole("heading", { name: "Review", exact: true })).toBeInTheDocument();
  expect(removed).toHaveAttribute("aria-pressed", "false");
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
});

it("keeps filtered references out of details when switching objects in the sidebar", async () => {
  render(<StructureChangesView binding={binding()} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  const sidebar = screen.getByRole("complementary", { name: "结构详情" });
  fireEvent.click(within(graph).getByRole("button", { name: /fn prepare_snapshot.*新增/ }));
  expect(within(sidebar).getAllByText(/code.calls → save_snapshot/)).toHaveLength(2);
  expect(sidebar).not.toHaveTextContent(/serde_json|verify_snapshot|Arc<T>|%2F|code.imports/);
  fireEvent.click(within(graph).getByRole("button", { name: /pub async fn run_review.*自身修改/ }));
  expect(within(sidebar).getByRole("heading", { name: "run_review" })).toBeInTheDocument();
  expect(sidebar).toHaveTextContent("generated::on_review_opened()");
  expect(within(sidebar).getAllByRole("listitem").every((item) => !item.textContent?.includes("generated::on_review_opened"))).toBe(true);
  expect(screen.getByTestId("rendered-graph")).toBe(graph);
});

it("presents long source, JSX relations and duplicate declarations in a mixed-language comparison", async () => {
  const comparison = structureMixedLanguageFixture();
  render(<StructureChangesView binding={binding(comparison)} dark={false} />);

  fireEvent.click(screen.getByRole("combobox", { name: "结构图语言" }));
  const languageOption = await screen.findByRole("option", { name: "TypeScript" });
  fireEvent.pointerDown(languageOption);
  fireEvent.pointerUp(languageOption);
  fireEvent.click(languageOption);
  await screen.findByRole("button", { name: /ReviewWorkspaceWith/ });
  const graph = screen.getByTestId("rendered-graph");
  fireEvent.click(within(graph).getByRole("button", { name: /ReviewWorkspaceWithAnExtraordinarilyLongName.*自身修改/ }));
  const details = screen.getByRole("complementary", { name: "结构详情" });
  expect(details).not.toHaveTextContent("code.calls → formatReview");
  expect(details).toHaveTextContent("JSX 使用组件 → ReviewSummaryCard");
  expect(screen.getByTestId("source-diff")).toHaveAttribute("data-original-language", "typescript");
  expect(screen.getByTestId("modified-source").textContent).toBe(comparison.after.nodes.find((node) => node.id === "typescript-component")!.source);

  fireEvent.click(screen.getByRole("button", { name: "显示选项" }));
  fireEvent.click(await screen.findByRole("menuitemcheckbox", { name: "显示全部对象" }));
  await waitFor(() => expect(within(graph).getAllByRole("button", { name: "PluginRegistry", exact: true })).toHaveLength(2));
});

it("clears selection for a new Snapshot and retains the sidebar for an empty visible diff", async () => {
  const comparison = structureReviewFixture();
  const view = render(<StructureChangesView binding={binding(comparison)} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  fireEvent.click(within(graph).getByRole("button", { name: /fn prepare_snapshot.*新增/ }));
  const next: StructureComparison = { ...structureHiddenReferencesFixture(), snapshotId: "next-snapshot" };
  view.rerender(<StructureChangesView binding={binding(next)} dark={false} />);
  const sidebar = screen.getByRole("complementary", { name: "结构详情" });
  expect(within(sidebar).getByText("选择图中的对象，在此查看详情。")).toBeInTheDocument();
  await waitFor(() => expect(screen.getByText("没有可显示的结构变化")).toBeInTheDocument());
  expect(screen.getByRole("separator", { name: "调整结构详情宽度" })).toBeInTheDocument();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

it("isolates member ports within a shared class card and hides relationships when selection is cleared", async () => {
  const comparison = structureClassDiagramFixture();
  const recursive = { semantic: "code.calls", target: { kind: "node" as const, id: "dense-method-0" } };
  comparison.after.nodes.find((node) => node.id === "dense-method-0")!.references.push(recursive);
  comparison.diff.nodes.find((node) => node.id === "dense-method-0")!.references.push({ kind: "added", value: recursive });
  render(<StructureChangesView binding={binding(comparison)} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
  fireEvent.click(within(graph).getByRole("button", { name: /pub fn open\(/ }));
  expect(screen.getAllByTestId("visible-relation").map((edge) => edge.textContent).sort()).toEqual([
    "dense-method-0 → dense-method-0 · + code.calls", "dense-method-0 → dense-method-1 · + code.calls",
  ]);
  fireEvent.click(within(graph).getByRole("button", { name: /pub fn close\(/ }));
  expect(screen.getAllByTestId("visible-relation").map((edge) => edge.textContent)).toEqual(["dense-method-0 → dense-method-1 · + code.calls"]);
  fireEvent.click(within(graph).getByRole("button", { name: "Graph background" }));
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
  expect(within(screen.getByRole("complementary", { name: "结构详情" })).getByText("选择图中的对象，在此查看详情。")).toBeInTheDocument();
  expect(within(graph).getByRole("button", { name: /pub fn close\(/ })).toHaveAttribute("aria-pressed", "false");
  expect(screen.getByTestId("rendered-graph")).toBe(graph);
});

it("selects the toggled object, retains nested folds and never restarts analysis or the canvas", async () => {
  const layoutCalls = vi.spyOn(graphPresentation, "layoutStructureComparison");
  const current = binding();
  render(<StructureChangesView binding={current} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  expect(layoutCalls).toHaveBeenCalledTimes(1);
  const sidebar = screen.getByRole("complementary", { name: "结构详情" });
  fireEvent.click(within(graph).getByRole("button", { name: "收起 Review", exact: true }));
  expect(layoutCalls).toHaveBeenCalledTimes(1);
  expect(within(screen.getByTestId("structure-canvas")).queryByRole("status")).not.toBeInTheDocument();
  expect(within(sidebar).getByRole("heading", { name: "Review", exact: true })).toBeInTheDocument();
  await waitFor(() => expect(within(graph).getByRole("button", { name: "展开 Review", exact: true })).toHaveAttribute("aria-expanded", "false"));
  expect(within(graph).queryByRole("button", { name: /pub async fn run_review/ })).not.toBeInTheDocument();
  expect(within(graph).getByText("内部 2 项变更")).toBeInTheDocument();
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
  fireEvent.click(within(graph).getByRole("button", { name: /fn prepare_snapshot.*新增/ }));
  expect(screen.getByTitle("Review::run_review")).toHaveTextContent("Review → new · + code.calls");

  fireEvent.click(within(graph).getByRole("button", { name: "收起 review", exact: true }));
  expect(within(sidebar).getByRole("heading", { name: "review", exact: true })).toBeInTheDocument();
  await waitFor(() => expect(within(graph).queryByRole("button", { name: "Review", exact: true })).not.toBeInTheDocument());
  fireEvent.click(within(graph).getByRole("button", { name: "展开 review", exact: true }));
  await waitFor(() => expect(within(graph).getByRole("button", { name: "展开 Review", exact: true })).toBeInTheDocument());
  expect(within(graph).queryByRole("button", { name: /pub async fn run_review/ })).not.toBeInTheDocument();
  fireEvent.click(within(graph).getByRole("button", { name: "展开 Review", exact: true }));
  expect(within(sidebar).getByRole("heading", { name: "Review", exact: true })).toBeInTheDocument();
  await waitFor(() => expect(within(graph).getByRole("button", { name: /pub async fn run_review/ })).toBeInTheDocument());
  expect(screen.getByTestId("rendered-graph")).toBe(graph);
  expect(current.retry).not.toHaveBeenCalled();
  expect(current.setOptions).not.toHaveBeenCalled();
  expect(layoutCalls).toHaveBeenCalledTimes(1);
});

it("offers collapse all and expand all while keeping context filters unchanged", async () => {
  const layoutCalls = vi.spyOn(graphPresentation, "layoutStructureComparison");
  render(<StructureChangesView binding={binding()} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  fireEvent.click(screen.getByRole("button", { name: "显示选项" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "全部收起" }));
  await waitFor(() => expect(within(graph).getByRole("button", { name: "展开 narrant", exact: true })).toBeInTheDocument());
  expect(within(graph).queryByRole("button", { name: "Review", exact: true })).not.toBeInTheDocument();
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "显示选项" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "全部展开" }));
  await waitFor(() => expect(within(graph).getByRole("button", { name: /pub async fn run_review/ })).toBeInTheDocument());
  expect(within(graph).queryByRole("button", { name: /pub fn restore\(/ })).not.toBeInTheDocument();
  expect(screen.getByTestId("rendered-graph")).toBe(graph);
  expect(layoutCalls).toHaveBeenCalledTimes(1);
});

it("optionally rearranges folded containers and restores the expanded layout without reanalysis", async () => {
  const layoutCalls = vi.spyOn(graphPresentation, "layoutStructureComparison");
  const compactCalls = vi.spyOn(graphPresentation, "relayoutStructureLayout");
  const current = binding(structureFileModulesFixture());
  render(<StructureChangesView binding={current} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  const fullGeometry = graph.dataset.layout;
  fireEvent.click(within(graph).getByRole("button", { name: "收起 src/review/initialization", exact: true }));
  const fixedGeometry = graph.dataset.layout;
  expect(compactCalls).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "显示选项" }));
  const option = await screen.findByRole("menuitemcheckbox", { name: "折叠时重新布局" });
  expect(option).toHaveAttribute("aria-checked", "false");
  await toggleRearrangement();
  await waitFor(() => expect(screen.queryByText("正在排列结构图")).not.toBeInTheDocument());
  expect(compactCalls).toHaveBeenCalledTimes(1);
  expect(graph.dataset.layout).not.toBe(fixedGeometry);
  expect(within(graph).getByRole("button", { name: "展开 src/review/initialization", exact: true })).toBeInTheDocument();
  fireEvent.click(within(graph).getByRole("button", { name: "export function start() · 新增", exact: true }));
  expect(screen.getByTitle("prepare")).toHaveTextContent("+ code.calls");
  fireEvent.click(screen.getByRole("button", { name: "显示选项" }));
  expect(await screen.findByRole("menuitemcheckbox", { name: "折叠时重新布局" })).toHaveAttribute("aria-checked", "true");
  await toggleRearrangement();
  expect(graph.dataset.layout).toBe(fixedGeometry);
  fireEvent.click(within(graph).getByRole("button", { name: "展开 src/review/initialization", exact: true }));
  expect(graph.dataset.layout).toBe(fullGeometry);

  await toggleRearrangement();
  fireEvent.click(screen.getByRole("button", { name: "显示选项" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "全部收起" }));
  await waitFor(() => expect(screen.queryByText("正在排列结构图")).not.toBeInTheDocument());
  expect(within(graph).getByRole("button", { name: "展开 src", exact: true })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "显示选项" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "全部展开" }));
  expect(graph.dataset.layout).toBe(fullGeometry);
  expect(layoutCalls).toHaveBeenCalledTimes(1);
  expect(screen.getByTestId("rendered-graph")).toBe(graph);
  expect(current.retry).not.toHaveBeenCalled();
  expect(current.setOptions).not.toHaveBeenCalled();
});

it("cancels pending rearrangement when disabled and ignores its late result", async () => {
  let complete!: (layout: StructureLayout) => void;
  const compactCalls = vi.spyOn(graphPresentation, "relayoutStructureLayout").mockImplementationOnce(() => new Promise((resolve) => { complete = resolve; }));
  render(<StructureChangesView binding={binding()} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  fireEvent.click(within(graph).getByRole("button", { name: "收起 Review", exact: true }));
  const fixedGeometry = graph.dataset.layout;
  await toggleRearrangement();
  expect(screen.getByText("正在排列结构图")).toBeInTheDocument();
  const [input, signal] = compactCalls.mock.calls[0];
  await toggleRearrangement();
  expect(signal?.aborted).toBe(true);
  expect(screen.queryByText("正在排列结构图")).not.toBeInTheDocument();
  await act(async () => { complete({ ...input, nodes: [], edges: [] }); });
  expect(graph.dataset.layout).toBe(fixedGeometry);
  expect(within(graph).getByRole("button", { name: "展开 Review", exact: true })).toBeInTheDocument();
});

it("selects directories without inventing source diffs and preserves file folds and exact cross-directory relationships", async () => {
  const layoutCalls = vi.spyOn(graphPresentation, "layoutStructureComparison");
  const current = binding(structureFileModulesFixture());
  render(<StructureChangesView binding={current} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  const sidebar = screen.getByRole("complementary", { name: "结构详情" });
  const path = "src/review/initialization";
  const filePath = `${path}/prepare-review-workspace-for-selected-repository.ts`;
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
  fireEvent.click(within(graph).getByRole("button", { name: filePath, exact: true }));
  expect(within(sidebar).getByTestId("modified-source")).toHaveTextContent("export function prepare()");
  fireEvent.click(within(graph).getByRole("button", { name: `收起 ${filePath}`, exact: true }));
  fireEvent.click(within(graph).getByRole("button", { name: `收起 ${path}`, exact: true }));
  expect(within(sidebar).getByRole("heading", { name: "目录详情", exact: true })).toBeInTheDocument();
  expect(within(sidebar).getByText("prepare-review-workspace-for-selected-repository.ts", { exact: true })).toBeInTheDocument();
  expect(within(sidebar).queryByTestId("source-diff")).not.toBeInTheDocument();
  expect(within(sidebar).queryByText("关系变化", { exact: true })).not.toBeInTheDocument();
  expect(within(graph).getByRole("button", { name: path, exact: true })).toHaveAttribute("aria-pressed", "true");
  expect(within(graph).queryByRole("button", { name: filePath, exact: true })).not.toBeInTheDocument();
  fireEvent.click(within(graph).getByRole("button", { name: "export function start() · 新增", exact: true }));
  expect(screen.getAllByTestId("visible-relation")).toHaveLength(2);
  expect(screen.getByTitle("prepare")).toHaveTextContent("+ code.calls");
  fireEvent.click(within(graph).getByRole("button", { name: `展开 ${path}`, exact: true }));
  expect(within(graph).getByRole("button", { name: `展开 ${filePath}`, exact: true })).toBeInTheDocument();
  expect(within(graph).queryByRole("button", { name: "export function prepare() · 新增", exact: true })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "显示选项" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "全部收起" }));
  expect(within(graph).getByRole("button", { name: "展开 src", exact: true })).toBeInTheDocument();
  expect(within(graph).queryByRole("button", { name: "src/startup.ts", exact: true })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "显示选项" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "全部展开" }));
  expect(within(graph).getByRole("button", { name: "export function prepare() · 新增", exact: true })).toBeInTheDocument();
  expect(screen.getByTestId("rendered-graph")).toBe(graph);
  expect(layoutCalls).toHaveBeenCalledTimes(1);
  expect(current.retry).not.toHaveBeenCalled();
  expect(current.setOptions).not.toHaveBeenCalled();
});

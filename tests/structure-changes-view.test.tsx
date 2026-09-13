import type { ComponentType, ReactNode } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/lib/i18n";
import StructureChangesView from "@/components/structure-review/structure-changes-view";
import { structureClassDiagramFixture, structureFieldTypesFixture, structureHiddenReferencesFixture, structureReviewFixture, structureTestCodeFixture } from "@/dev/structure-review-fixture";
import type { ObjectData, ObjectNode, StructureEdge } from "@/structure-review/graph-presentation";
import type { StructureBinding, StructureComparison } from "@/structure-review/types";

// jsdom 没有画布尺寸；保留真实布局和节点交互，只替换 React Flow 的浏览器绘制层。
vi.mock("@xyflow/react", async (importOriginal) => ({
  ...await importOriginal<typeof import("@xyflow/react")>(),
  ReactFlow: ({ nodes, edges, nodeTypes, children, onPaneClick }: { nodes: ObjectNode[]; edges: StructureEdge[]; nodeTypes: { codeObject: ComponentType<{ data: ObjectData }> }; children: ReactNode; onPaneClick(): void }) => {
    const ports = new Map(nodes.flatMap((node) => node.data.ports.map((port) => [port.id, port.objectId] as const)));
    return <div data-testid="rendered-graph">{nodes.map((node) => <nodeTypes.codeObject key={node.id} data={node.data} />)}{children}
      {edges.map((edge) => <div key={edge.id} data-testid="visible-relation">{ports.get(edge.sourceHandle!)} → {ports.get(edge.targetHandle!)} · {edge.label}</div>)}
      <button onClick={onPaneClick}>Graph background</button>
    </div>;
  },
  Handle: () => null,
  Background: () => null,
  Controls: () => null,
}));

/** 用完整的比较结果驱动公开结构视图接口。 */
function binding(comparison = structureReviewFixture()): StructureBinding {
  return { state: { kind: "ready", comparison }, options: { includeTests: false }, setOptions: vi.fn(), retry: vi.fn(), cancel: vi.fn() };
}

beforeEach(async () => { await i18n.changeLanguage("zh-CN"); });
afterEach(cleanup);

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

it("shows coverage in the same sidebar and keeps filtered references out of object details", async () => {
  render(<StructureChangesView binding={binding()} dark={false} />);
  const graph = await screen.findByTestId("rendered-graph");
  const sidebar = screen.getByRole("complementary", { name: "结构详情" });
  fireEvent.click(within(graph).getByRole("button", { name: /fn prepare_snapshot.*新增/ }));
  expect(within(sidebar).getAllByText(/code.calls → save_snapshot/)).toHaveLength(2);
  expect(sidebar).not.toHaveTextContent(/serde_json|verify_snapshot|Arc<T>|%2F|code.imports/);
  fireEvent.click(screen.getByRole("button", { name: "部分覆盖 · 2 / 3 个文件" }));
  expect(within(sidebar).getByRole("heading", { name: "分析覆盖范围" })).toBeInTheDocument();
  expect(within(sidebar).getByText("src/review.rs")).toBeInTheDocument();
  expect(within(sidebar).getByText("src/structure-changes-view.tsx")).toBeInTheDocument();
  expect(within(graph).getByRole("button", { name: /fn prepare_snapshot.*新增/ })).toHaveAttribute("aria-pressed", "false");
  expect(screen.queryAllByTestId("visible-relation")).toHaveLength(0);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  fireEvent.click(within(graph).getByRole("button", { name: /pub async fn run_review.*自身修改/ }));
  expect(within(sidebar).getByRole("heading", { name: "run_review" })).toBeInTheDocument();
  expect(sidebar).toHaveTextContent("generated::on_review_opened()");
  expect(within(sidebar).getAllByRole("listitem").every((item) => !item.textContent?.includes("generated::on_review_opened"))).toBe(true);
  expect(screen.getByTestId("rendered-graph")).toBe(graph);
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

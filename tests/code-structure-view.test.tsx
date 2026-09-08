import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CodeStructureView } from "@/components/code-structure/code-structure-view";
import { createStructureGraphPresentation } from "@/review/structure-graph";
import { createStructurePreviewResult } from "@/dev/scenarios/structure-preview-data";
import { i18n } from "@/lib/i18n";
import type { StructureResult } from "@/review/structure-types";

afterEach(cleanup);

/** 从公共受控组件驱动交互，无需模拟内部拓扑或关系查询。 */
function Harness({ result, onNavigate = vi.fn(), onPresentationChange }: { result: StructureResult; onNavigate?: Parameters<typeof CodeStructureView>[0]["onNavigate"]; onPresentationChange?: Parameters<typeof CodeStructureView>[0]["onPresentationChange"] }) {
  const [presentation, setPresentation] = useState(() => createStructureGraphPresentation(result));
  return <div style={{ width: 1200, height: 720 }}><CodeStructureView analysis={{ status: "completed", result }} presentation={presentation} onPresentationChange={(next) => { setPresentation(next); onPresentationChange?.(next); }} onNavigate={onNavigate} /></div>;
}

describe("Code Structure View", () => {
  it("keeps a relationship-only change accessible with type lines off and emits occurrence rather than target navigation", async () => {
    await i18n.changeLanguage("en");
    const result = createStructurePreviewResult("relationship-only");
    const onNavigate = vi.fn();
    render(<Harness result={result} onNavigate={onNavigate} />);
    expect(screen.getByText(/0 declaration changes · 1 relationship changes/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /View occurrences/ }));
    expect(onNavigate).toHaveBeenLastCalledWith({ kind: "relationship", relationshipId: "relation:refresh:welcome" });
    fireEvent.click(screen.getByRole("button", { name: "View target definition" }));
    expect(onNavigate).toHaveBeenLastCalledWith({ kind: "target-definition", relationshipId: "relation:refresh:welcome", nodeId: "decl:welcome" });
    expect(screen.getByText("↳ Inside anonymous callback")).toBeInTheDocument();
  }, 120_000);
  it("searches outside the initial display batch and offers the precise matched declaration", async () => {
    await i18n.changeLanguage("en");
    const onNavigate = vi.fn();
    render(<Harness result={createStructurePreviewResult("large")} onNavigate={onNavigate} />);
    expect(screen.getByRole("button", { name: "Show 60 more" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Search all analyzed structures" }), { target: { value: "indexed_operation_179" } });
    expect(screen.getByText("Full analysis search: 1 matches")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "indexed_operation_179" }));
    expect(onNavigate).toHaveBeenLastCalledWith({ kind: "node", nodeId: "decl:bulk:179" });
  }, 120_000);
  it("loads every pending batch through the visible control and keeps newly displayed nodes navigable", async () => {
    await i18n.changeLanguage("en");
    const onNavigate = vi.fn();
    render(<Harness result={createStructurePreviewResult("large")} onNavigate={onNavigate} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Search all analyzed structures" }), { target: { value: "indexed_operation_" } });
    expect(screen.getByText("Full analysis search: 180 matches")).toBeInTheDocument();
    expect(screen.getByText("Showing 61 / 181 · 120 pending")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "indexed_operation_060" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Show 60 more" }));
    expect(screen.getByText("Showing 121 / 181 · 60 pending")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "indexed_operation_060" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "indexed_operation_179" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Show 60 more" }));
    expect(screen.getByText("Showing 181 / 181 · 0 pending")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Show 60 more" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "indexed_operation_179" }));
    expect(onNavigate).toHaveBeenLastCalledWith({ kind: "node", nodeId: "decl:bulk:179" });
  }, 120_000);
  it("stores manual zoom intent and returns to automatic sizing only when Fit view is requested", async () => {
    await i18n.changeLanguage("en");
    const onPresentationChange = vi.fn();
    render(<Harness result={createStructurePreviewResult()} onPresentationChange={onPresentationChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    expect(onPresentationChange).toHaveBeenLastCalledWith(expect.objectContaining({ viewportMode: "manual" }));
    fireEvent.click(screen.getByRole("button", { name: "Fit view" }));
    expect(onPresentationChange).toHaveBeenLastCalledWith(expect.objectContaining({ viewportMode: "auto" }));
  }, 120_000);
});

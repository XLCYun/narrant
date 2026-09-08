import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StructurePeek, type StructurePeekProps } from "@/components/code-structure/structure-peek";
import { createStructurePeekPreview } from "@/dev/scenarios/structure-peek-preview";
import { resolveStructurePeekNavigation } from "@/review/structure-peek";
import { i18n } from "@/lib/i18n";

const editors = vi.hoisted(() => ({ diff: vi.fn(), single: vi.fn() }));
vi.mock("@monaco-editor/react", () => ({
  loader: { config: vi.fn(), init: () => Promise.resolve({}) },
  DiffEditor: (props: unknown) => { editors.diff(props); return <div data-testid="full-diff" />; },
  Editor: (props: unknown) => { editors.single(props); return <div data-testid="readable-source" />; },
}));

/** 真实来源契约驱动受控组件，Monaco 只替换其公开的渲染边界。 */
function propsFor(scenario: Parameters<typeof createStructurePeekPreview>[0] = "changed"): StructurePeekProps {
  const fixture = createStructurePeekPreview(scenario);
  const selection = resolveStructurePeekNavigation(fixture.result, fixture.intent)!;
  return {
    state: { selection, locationIndex: 0, requestId: 1, load: { status: "ready", content: fixture.contents.find((content) => content.sourceId === selection.locations[0]?.sourceId) ?? fixture.contents[0] } },
    narrow: false, diffMode: "side-by-side", editorTheme: "light", viewState: null,
    onSaveViewState: vi.fn(), onSelectLocation: vi.fn(), onNavigate: vi.fn(), onRetry: vi.fn(), onClose: vi.fn(), onOpenFileChange: vi.fn(),
  };
}

/** 通过公开来源菜单模拟完整鼠标选择，包含按下、释放和点击。 */
function chooseSourceLocation(option: RegExp) {
  fireEvent.click(screen.getByRole("combobox", { name: "Source locations" }));
  const item = screen.getByRole("option", { name: option });
  fireEvent.pointerDown(item, { pointerType: "mouse", button: 0 });
  fireEvent.pointerUp(item, { pointerType: "mouse", button: 0 });
  fireEvent.click(item, { button: 0, detail: 1 });
}

beforeEach(async () => { editors.diff.mockClear(); editors.single.mockClear(); await i18n.changeLanguage("en"); });
afterEach(cleanup);

describe("受控 Structure Peek", () => {
  it("把完整长源码交给 Diff，窄窗口使用单栏，并保持真实 oldPath/path", () => {
    const props = propsFor("long");
    render(<StructurePeek {...props} narrow />);
    expect(editors.diff.mock.lastCall?.[0]).toMatchObject({
      original: expect.stringContaining("// 完整文件上下文 1\r\n"),
      modified: expect.stringContaining("// 后续上下文 160"),
      options: { renderSideBySide: false },
    });
    expect(props.onOpenFileChange).not.toHaveBeenCalled();
  });

  it("真实重命名在来源列表保留旧路径，显式按钮打开真实新路径", () => {
    const props = propsFor("renamed");
    render(<StructurePeek {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Open in File Changes" }));
    expect(props.onOpenFileChange).toHaveBeenCalledWith("renamed/welcome.ts");
    fireEvent.click(screen.getByRole("combobox", { name: "Source locations" }));
    expect(screen.getByRole("option", { name: /Before.*welcome.ts/ })).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("option", { name: /Before.*welcome.ts/ }), { key: "Escape" });
    expect(props.onClose).not.toHaveBeenCalled();
  });

  it("移动来源菜单发出选择动作，各自展示真实完整文件对而不拼接虚拟 File Change", () => {
    const fixture = createStructurePeekPreview("moved");
    const props = propsFor("moved");
    const rendered = render(<StructurePeek {...props} />);
    expect(editors.diff.mock.lastCall?.[0]).toMatchObject({ original: "// Destination previously contained no welcome function.\n", modified: expect.stringContaining('console.log("新代码🙂")') });
    chooseSourceLocation(/^welcome\.ts:121 · Before · Declaration/);
    expect(props.onSelectLocation).toHaveBeenCalledWith(1);
    expect(props.onOpenFileChange).not.toHaveBeenCalled();
    expect(props.onNavigate).not.toHaveBeenCalled();
    // 受控上层提交所选来源后，只展示 welcome.ts 自己的 before/after。
    rendered.rerender(<StructurePeek {...props} state={{ ...props.state!, locationIndex: 1, requestId: 2, load: { status: "ready", content: fixture.contents[0] } }} />);
    expect(editors.diff.mock.lastCall?.[0]).toMatchObject({ original: expect.stringContaining('console.log("删除🙂")'), modified: "// The function moved away from this file.\n" });
    fireEvent.click(screen.getByRole("button", { name: "Open in File Changes" }));
    expect(props.onOpenFileChange).toHaveBeenLastCalledWith("welcome.ts");
    chooseSourceLocation(/^destination\.ts:121 · After · Declaration/);
    expect(props.onSelectLocation).toHaveBeenLastCalledWith(0);
    expect(props.onOpenFileChange).toHaveBeenCalledTimes(1);
    rendered.rerender(<StructurePeek {...props} state={{ ...props.state!, requestId: 3, load: { status: "ready", content: fixture.contents[1] } }} />);
    expect(editors.diff.mock.lastCall?.[0]).toMatchObject({ original: "// Destination previously contained no welcome function.\n", modified: expect.stringContaining('console.log("新代码🙂")') });
    fireEvent.click(screen.getByRole("button", { name: "Open in File Changes" }));
    expect(props.onOpenFileChange).toHaveBeenLastCalledWith("destination.ts");
  });

  it("长路径多 impl 来源以可辨识的文件名开头，保留完整路径并选择真实位置", () => {
    const props = propsFor("multiple-impls");
    render(<StructurePeek {...props} />);
    fireEvent.click(screen.getByRole("combobox", { name: "Source locations" }));
    const refresh = screen.getByRole("option", { name: /^refresh_review_access\.rs:3 · After · Implementation/ });
    const persist = screen.getByRole("option", { name: /^persist_snapshot_state\.rs:3 · After · Implementation/ });
    expect(within(refresh).getByText("src/review/lifecycle/representative_sources/long_module_names_for_responsive_source_navigation/implementations/refresh_review_access.rs")).toBeInTheDocument();
    expect(within(persist).getByText("src/review/lifecycle/representative_sources/long_module_names_for_responsive_source_navigation/implementations/persist_snapshot_state.rs")).toBeInTheDocument();
    fireEvent.pointerDown(persist, { pointerType: "mouse", button: 0 });
    fireEvent.pointerUp(persist, { pointerType: "mouse", button: 0 });
    fireEvent.click(persist, { button: 0, detail: 1 });
    expect(props.onSelectLocation).toHaveBeenCalledWith(4);
    expect(props.onOpenFileChange).not.toHaveBeenCalled();
  });

  it("一侧不可用时显示原因，另一侧仍是完整阅读器，不能冒充空文件 Diff", () => {
    render(<StructurePeek {...propsFor("unavailable")} />);
    expect(screen.getByText(/before Git object cannot be read/)).toBeInTheDocument();
    expect(screen.getByTestId("readable-source")).toBeInTheDocument();
    expect(editors.single.mock.lastCall?.[0]).toMatchObject({ value: expect.stringContaining("export function helper() {}") });
    expect(editors.diff).not.toHaveBeenCalled();
  });

  it("明确区分 Git 缺侧和真实零字节文件，新增与删除文件都保留另一侧完整文本", () => {
    const rendered = render(<StructurePeek {...propsFor("file-added")} />);
    expect(screen.getByText("Before does not exist in this Snapshot.")).toBeInTheDocument();
    expect(editors.diff.mock.lastCall?.[0]).toMatchObject({ original: "", modified: expect.stringContaining("export function helper() {}") });
    const empty = propsFor("empty-file");
    rendered.rerender(<StructurePeek {...empty} />);
    // Monaco 原文同样为空，但真实存在的空文件既不显示缺侧提示，也不丢掉旧侧入口。
    expect(screen.queryByText(/does not exist in this Snapshot/)).not.toBeInTheDocument();
    expect(screen.queryByText(/source unavailable/)).not.toBeInTheDocument();
    expect(editors.diff.mock.lastCall?.[0]).toMatchObject({ original: "", modified: expect.stringContaining("export function helper() {}") });
    chooseSourceLocation(/^welcome\.ts:1 · Before · Declaration/);
    expect(empty.onSelectLocation).toHaveBeenCalledWith(1);
    rendered.rerender(<StructurePeek {...propsFor("file-deleted")} />);
    expect(screen.getByText("After does not exist in this Snapshot.")).toBeInTheDocument();
    expect(editors.diff.mock.lastCall?.[0]).toMatchObject({ original: expect.stringContaining('export const previousContext = "完整旧文件🙂";'), modified: "" });
    expect(editors.single).not.toHaveBeenCalled();
  });

  it("未修改上下文和外部定义都不生成 File Change 跳转", () => {
    const rendered = render(<StructurePeek {...propsFor("unchanged")} />);
    expect(screen.getByText("Unchanged")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open in File Changes" })).not.toBeInTheDocument();
    rendered.rerender(<StructurePeek {...propsFor("external")} />);
    expect(screen.getByText("This external target has no indexed local definition.")).toBeInTheDocument();
    expect(screen.queryByTestId("full-diff")).not.toBeInTheDocument();
  });

  it("忽略其他 Snapshot 的已读内容，键盘关闭只回调拥有焦点的上层", () => {
    const props = propsFor();
    if (props.state!.load.status === "ready") props.state!.load.content.snapshotId = "stale";
    render(<StructurePeek {...props} />);
    expect(screen.getByText("Reading complete source…")).toBeInTheDocument();
    expect(editors.diff).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByRole("region", { name: "Source Peek" }), { key: "Escape" });
    expect(props.onClose).toHaveBeenCalledWith("keyboard");
  });
});

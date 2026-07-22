import { afterEach, describe, it, expect, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@monaco-editor/react", () => ({ DiffEditor: () => null }));
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { App } from "@/App";

afterEach(() => {
  cleanup();
  invoke.mockReset();
});

describe("App", () => {
  it("渲染 Narrant 标题与本地分支比较入口", () => {
    invoke.mockResolvedValue([]);
    render(<App />);
    expect(screen.getByRole("heading", { name: "Narrant" })).toBeInTheDocument();
    expect(screen.getByText("本地分支比较")).toBeInTheDocument();
  });

  it("展示完整文件树并按需加载所选 File Change", async () => {
    invoke
      .mockResolvedValueOnce(["base", "origin/topic"])
      .mockResolvedValueOnce({
        comparisonBase: "base-sha",
        headRevision: "head-sha",
        fileChanges: [
          { path: "src/main.rs", oldPath: null, status: "modified", contentKind: "text", beforeSize: 3, afterSize: 4, before: null, after: null },
          { path: "assets/logo.bin", oldPath: null, status: "added", contentKind: "binary", beforeSize: null, afterSize: 128, before: null, after: null },
        ],
      })
      .mockResolvedValueOnce({ path: "assets/logo.bin", oldPath: null, status: "added", contentKind: "binary", beforeSize: null, afterSize: 128, before: null, after: null });
    render(<App />);

    fireEvent.change(screen.getByLabelText("本地仓库路径"), { target: { value: "/repo" } });
    fireEvent.click(screen.getByRole("button", { name: "读取分支" }));
    await waitFor(() => expect(screen.getByLabelText("Head Ref")).toHaveTextContent("origin/topic"));
    fireEvent.click(screen.getByRole("button", { name: "打开 Review Snapshot" }));

    expect(await screen.findByRole("tree", { name: "文件树" })).toBeInTheDocument();
    expect(screen.getByRole("treeitem", { name: /src/ })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("treeitem", { name: /main.rs.*modified/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("treeitem", { name: /logo.bin.*added/ }));

    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("read_local_file_change", {
      repositoryPath: "/repo",
      comparisonBase: "base-sha",
      headRevision: "head-sha",
      path: "assets/logo.bin",
    }));
    expect(await screen.findByText("二进制文件（变更前：无；变更后：128 字节）")).toBeInTheDocument();
  });
});

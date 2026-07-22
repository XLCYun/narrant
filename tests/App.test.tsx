import { afterEach, describe, it, expect, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@monaco-editor/react", () => ({
  DiffEditor: ({ theme }: { theme?: string }) => <div data-testid="diff-editor" data-theme={theme} />,
}));
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const openUrl = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));

import { App } from "@/App";

afterEach(() => {
  cleanup();
  invoke.mockReset();
  openUrl.mockReset();
  document.documentElement.classList.remove("dark");
});

function mockSystemTheme(initialDark: boolean) {
  let matches = initialDark;
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  vi.stubGlobal("matchMedia", vi.fn((query: string) => ({
    media: query,
    get matches() { return matches; },
    onchange: null,
    addEventListener: (_type: "change", listener: (event: MediaQueryListEvent) => void) => listeners.add(listener),
    removeEventListener: (_type: "change", listener: (event: MediaQueryListEvent) => void) => listeners.delete(listener),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })));

  return (dark: boolean) => {
    matches = dark;
    const event = { matches: dark, media: "(prefers-color-scheme: dark)" } as MediaQueryListEvent;
    listeners.forEach((listener) => listener(event));
  };
}

describe("App", () => {
  it("初始浅色系统主题不设置 dark class", () => {
    mockSystemTheme(false);
    render(<App />);
    expect(document.documentElement).not.toHaveClass("dark");
  });

  it("初始深色系统主题设置 dark class", () => {
    mockSystemTheme(true);
    render(<App />);
    expect(document.documentElement).toHaveClass("dark");
  });

  it("运行时系统主题变化会同步根元素", () => {
    const changeSystemTheme = mockSystemTheme(false);
    render(<App />);

    changeSystemTheme(true);
    expect(document.documentElement).toHaveClass("dark");
    changeSystemTheme(false);
    expect(document.documentElement).not.toHaveClass("dark");
  });

  it("渲染 Narrant 标题与本地分支比较入口", () => {
    invoke.mockResolvedValue([]);
    render(<App />);
    expect(screen.getByRole("heading", { name: "Narrant" })).toBeInTheDocument();
    expect(screen.getByText("本地分支比较")).toBeInTheDocument();
  });

  it("匿名打开公开 Pull Request 并安全展示固定 Review Snapshot", async () => {
    invoke.mockResolvedValueOnce({
      owner: "octo-org", repository: "octo-repo", title: "Improve greeting", number: 42,
      author: "octocat", state: "merged", draft: false, baseRef: "main",
      baseRevision: "base-sha", comparisonBase: "base-sha", headRef: "feature",
      headRevision: "head-sha", description: "## Summary\n**Safe** <script>alert(1)</script>\n\n![tracker](https://tracker.invalid/pixel.png)\n\n[查看详情](https://example.com/details)",
      changedFiles: 1, additions: 5, deletions: 1, githubUrl: "https://github.com/octo-org/octo-repo/pull/42",
      fileChanges: [{ path: "src/main.rs", oldPath: null, status: "modified", contentKind: "text", beforeSize: null, afterSize: null, before: null, after: null }],
    });
    render(<App />);

    fireEvent.change(screen.getByLabelText("GitHub Pull Request URL"), { target: { value: "https://github.com/octo-org/octo-repo/pull/42" } });
    fireEvent.click(screen.getByRole("button", { name: "匿名打开" }));

    expect(await screen.findByRole("heading", { name: "Improve greeting" })).toBeInTheDocument();
    expect(screen.getByText("#42 · octocat · 已合并")).toBeInTheDocument();
    expect(screen.getByText("main @ base-sha → feature @ head-sha")).toBeInTheDocument();
    expect(screen.getByText("1 个文件 · +5 · −1")).toBeInTheDocument();
    expect(screen.getByText("Safe").tagName).toBe("STRONG");
    expect(document.querySelector("script")).not.toBeInTheDocument();
    expect(document.querySelector("img")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: "查看详情" }));
    expect(openUrl).toHaveBeenCalledWith("https://example.com/details");
    expect(screen.getByRole("treeitem", { name: /main.rs.*modified/ })).toBeInTheDocument();
  });

  it("超过 GitHub 文件上限时提供可在系统浏览器打开的恢复链接", async () => {
    mockSystemTheme(false);
    invoke.mockRejectedValueOnce("此 Pull Request 有 3001 个 File Change，超过 GitHub API 的 3000 个上限，无法保证审核完整。请在 GitHub 查看：https://github.com/octo-org/octo-repo/pull/42");
    render(<App />);

    fireEvent.change(screen.getByLabelText("GitHub Pull Request URL"), { target: { value: "https://github.com/octo-org/octo-repo/pull/42" } });
    fireEvent.click(screen.getByRole("button", { name: "匿名打开" }));

    const recoveryLink = await screen.findByRole("link", { name: "在 GitHub 查看" });
    fireEvent.click(recoveryLink);
    expect(openUrl).toHaveBeenCalledWith("https://github.com/octo-org/octo-repo/pull/42");
  });

  it("GitHub File Change 通过统一快照契约按需加载到 Monaco", async () => {
    mockSystemTheme(false);
    const snapshot = {
      owner: "octo-org", repository: "octo-repo", title: "Update readme", number: 8,
      author: "octocat", state: "open", draft: false, baseRef: "main", baseRevision: "base",
      comparisonBase: "base", headRef: "docs", headRevision: "head", description: "",
      changedFiles: 1, additions: 1, deletions: 1, githubUrl: "https://github.com/octo-org/octo-repo/pull/8",
      fileChanges: [{ path: "README.md", oldPath: null, status: "modified", contentKind: "text", beforeSize: null, afterSize: null, before: null, after: null }],
    };
    invoke
      .mockResolvedValueOnce(snapshot)
      .mockResolvedValueOnce({ ...snapshot.fileChanges[0], beforeSize: 4, afterSize: 4, before: "old\n", after: "new\n" });
    render(<App />);

    fireEvent.change(screen.getByLabelText("GitHub Pull Request URL"), { target: { value: snapshot.githubUrl } });
    fireEvent.click(screen.getByRole("button", { name: "匿名打开" }));
    fireEvent.click(await screen.findByRole("treeitem", { name: /README.md.*modified/ }));

    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("read_github_file_change", { snapshot, path: "README.md" }));
    expect(await screen.findByTestId("diff-editor")).toBeInTheDocument();
  });

  it("公开 Pull Request 没有 File Change 时显示简体中文空状态", async () => {
    mockSystemTheme(false);
    invoke.mockResolvedValueOnce({
      owner: "octo-org", repository: "octo-repo", title: "Empty", number: 9, author: "octocat",
      state: "closed", draft: false, baseRef: "main", baseRevision: "same", comparisonBase: "same",
      headRef: "topic", headRevision: "same", description: "", changedFiles: 0, additions: 0,
      deletions: 0, githubUrl: "https://github.com/octo-org/octo-repo/pull/9", fileChanges: [],
    });
    render(<App />);
    fireEvent.change(screen.getByLabelText("GitHub Pull Request URL"), { target: { value: "https://github.com/octo-org/octo-repo/pull/9" } });
    fireEvent.click(screen.getByRole("button", { name: "匿名打开" }));

    expect(await screen.findByText("没有 File Change")).toBeInTheDocument();
  });

  it("展示完整文件树并按需加载所选 File Change", async () => {
    mockSystemTheme(false);
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

  it("Monaco Diff Editor 跟随运行时系统主题", async () => {
    const changeSystemTheme = mockSystemTheme(false);
    invoke
      .mockResolvedValueOnce(["base", "head"])
      .mockResolvedValueOnce({
        comparisonBase: "base-sha",
        headRevision: "head-sha",
        fileChanges: [{ path: "readme.txt", oldPath: null, status: "modified", contentKind: "text", beforeSize: 3, afterSize: 4, before: null, after: null }],
      })
      .mockResolvedValueOnce({ path: "readme.txt", oldPath: null, status: "modified", contentKind: "text", beforeSize: 3, afterSize: 4, before: "old", after: "new" });
    render(<App />);

    fireEvent.change(screen.getByLabelText("本地仓库路径"), { target: { value: "/repo" } });
    fireEvent.click(screen.getByRole("button", { name: "读取分支" }));
    await waitFor(() => expect(screen.getByLabelText("Head Ref")).toHaveTextContent("head"));
    fireEvent.click(screen.getByRole("button", { name: "打开 Review Snapshot" }));
    fireEvent.click(await screen.findByRole("treeitem", { name: /readme.txt.*modified/ }));

    expect(await screen.findByTestId("diff-editor")).toHaveAttribute("data-theme", "light");
    changeSystemTheme(true);
    await waitFor(() => expect(screen.getByTestId("diff-editor")).toHaveAttribute("data-theme", "vs-dark"));
  });
});

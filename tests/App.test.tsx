import { afterEach, describe, it, expect, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@monaco-editor/react", () => ({
  DiffEditor: ({ theme }: { theme?: string }) => <div data-testid="diff-editor" data-theme={theme} />,
}));
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const openUrl = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));
const writeText = vi.hoisted(() => vi.fn());

import { App } from "@/App";

afterEach(() => {
  cleanup();
  invoke.mockReset();
  openUrl.mockReset();
  writeText.mockReset();
  document.documentElement.classList.remove("dark");
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
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
  it("从仓库优先首页添加 Repository Entry 并进入仓库页面", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 21,
      canonicalPath: "/work/narrant",
      commonDirectory: "/work/narrant/.git",
      createdAt: 1,
      lastOpenedAt: 1,
      available: true,
      unavailableReason: null,
    };
    invoke.mockImplementation((command: string) => {
      if (command === "list_repository_entries") return Promise.resolve([]);
      if (command === "register_repository_entry") return Promise.resolve(entry);
      if (command === "open_repository_entry") return Promise.resolve(entry);
      if (command === "repository_workspace_trust") return Promise.resolve(false);
      return Promise.resolve([]);
    });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "Repository Entries" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Repository 路径"), {
      target: { value: "/work/narrant" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加 Repository" }));
    fireEvent.click(await screen.findByRole("button", { name: "/work/narrant" }));

    expect(await screen.findByRole("heading", { name: "narrant" })).toBeInTheDocument();
    expect(screen.getByText("/work/narrant")).toBeInTheDocument();
    expect(screen.queryByLabelText("Repository Catalog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "返回 Repository Entries" })).toBeInTheDocument();
    expect(screen.getByLabelText("本地仓库路径")).toHaveValue("/work/narrant");
    expect(screen.getByLabelText("Workspace Trust 风险说明")).toHaveTextContent("hooks、filters、Git LFS");
    expect(screen.getByRole("button", { name: "打开 Review Snapshot" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "我了解风险并信任此 Repository Entry" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      "grant_repository_workspace_trust",
      { repositoryEntryId: 21 },
    ));
  });

  it("从首页最近列表和 Repository Entry 重新打开持久化 Review", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 23, canonicalPath: "/work/narrant", commonDirectory: "/work/narrant/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    const summary = {
      reviewId: "review-23", repositoryEntryId: 23, repositoryPath: "/work/narrant",
      baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: 20,
    };
    const newerSummary = {
      ...summary, reviewId: "review-24", baseRef: "refs/heads/release",
      headRef: "refs/heads/hotfix", lastOpenedAt: 30,
    };
    const snapshot = {
      reviewId: "review-23", source: "local", baseRef: summary.baseRef,
      baseRevision: "base", comparisonBase: "base", headRef: summary.headRef,
      headRevision: "head", fileChanges: [],
    };
    invoke.mockImplementation((command: string, arguments_?: Record<string, unknown>) => {
      if (command === "list_repository_entries") return Promise.resolve([entry]);
      if (command === "list_reviews") {
        return Promise.resolve(arguments_?.repositoryEntryId === 23
          ? [summary]
          : [newerSummary, summary]);
      }
      if (command === "open_repository_entry") return Promise.resolve(entry);
      if (command === "repository_workspace_trust") return Promise.resolve(true);
      if (command === "reopen_review") return Promise.resolve(snapshot);
      return Promise.resolve([]);
    });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /main → topic/ }));
    expect(await screen.findByText("没有 File Change")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("reopen_review", { reviewId: "review-23" });

    expect(await screen.findByRole("region", { name: "Repository Reviews" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /main → topic/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "返回 Repository Entries" }));
    const recentButtons = screen.getByRole("region", { name: "Recent Reviews" })
      .querySelectorAll("button");
    expect(recentButtons[0]).toHaveTextContent("main → topic");
  });

  it("按 GitHub repository 分组展示 Pull Requests，且保留局部失败", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 25, canonicalPath: "/work/narrant", commonDirectory: "/work/narrant/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    invoke.mockImplementation((command: string) => {
      if (command === "list_repository_entries") return Promise.resolve([entry]);
      if (command === "list_reviews") return Promise.resolve([]);
      if (command === "open_repository_entry") return Promise.resolve(entry);
      if (command === "discover_github_pull_requests") return Promise.resolve([
        {
          repository: { owner: "fork-owner", name: "fork-repo" },
          remoteNames: ["fork"], pullRequests: [], error: "GitHub 访问次数已用尽",
        },
        {
          repository: { owner: "octo-org", name: "octo-repo" },
          remoteNames: ["origin", "upstream"], error: null,
          pullRequests: [{
            number: 42, title: "Metadata only", description: "Details", author: "octocat",
            state: "open", draft: false,
            url: "https://github.com/octo-org/octo-repo/pull/42",
            base: { ref: "main", sha: "base-sha" },
            head: { ref: "feature", sha: "head-sha" },
          }],
        },
      ]);
      return Promise.resolve([]);
    });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "/work/narrant" }));

    expect(await screen.findByRole("heading", { name: "octo-org/octo-repo" })).toBeInTheDocument();
    expect(screen.getByText("remotes: origin, upstream")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /#42 Metadata only/ })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "fork-owner/fork-repo" })).toBeInTheDocument();
    expect(screen.getByText("GitHub 访问次数已用尽")).toBeInTheDocument();
  });

  it("保留并展示已移动 Repository Entry 的可恢复不可用状态", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockImplementation((command: string) => {
      if (command === "list_repository_entries") return Promise.resolve([{
        id: 22,
        canonicalPath: "/missing/repository",
        commonDirectory: "/missing/repository/.git",
        createdAt: 1,
        lastOpenedAt: 1,
        available: false,
        unavailableReason: "Repository Entry 路径不存在或不可访问",
      }]);
      return Promise.resolve();
    });
    render(<App />);

    expect(await screen.findByText("/missing/repository")).toBeInTheDocument();
    expect(screen.getByText("不可用")).toBeInTheDocument();
    expect(screen.getByText("Repository Entry 路径不存在或不可访问")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "/missing/repository" })).toBeDisabled();
  });

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
      sessionId: "session-github-42",
      source: "github",
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

  it("连接 GitHub 会先打开安装页，再引导设备码授权且不接收 token", async () => {
    mockSystemTheme(false);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    invoke.mockImplementation((command: string) => {
      if (command === "github_auth_status") return Promise.resolve({
        configured: true,
        signedIn: false,
        appInstallUrl: "https://github.com/apps/narrant/installations/new",
      });
      if (command === "start_github_authorization") return Promise.resolve({
        authorizationId: "authorization-1",
        userCode: "ABCD-EFGH",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5,
      });
      return Promise.resolve();
    });
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "连接 GitHub" }));
    expect(await screen.findByText(/组织仓库可能需要管理员审批/)).toBeInTheDocument();
    expect(openUrl).toHaveBeenCalledWith("https://github.com/apps/narrant/installations/new");
    fireEvent.click(screen.getByRole("button", { name: "我已安装，继续" }));
    expect(await screen.findByText("ABCD-EFGH")).toBeInTheDocument();
    expect(screen.getByText(/不要输入他人发送的验证码/)).toBeInTheDocument();
    expect(openUrl).toHaveBeenCalledWith("https://github.com/login/device");
    expect(screen.getByRole("link", { name: "https://github.com/login/device" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "复制设备码" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("ABCD-EFGH"));
    expect(screen.getByRole("button", { name: "设备码已复制" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "复制链接" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("https://github.com/login/device"));
    expect(screen.getByRole("button", { name: "链接已复制" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "打开 GitHub" }));
    expect(openUrl).toHaveBeenLastCalledWith("https://github.com/login/device");
    fireEvent.click(screen.getByRole("button", { name: "我已在 GitHub 完成授权" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      "complete_github_authorization",
      { authorizationId: "authorization-1" },
    ));
    expect(await screen.findByRole("button", { name: "退出 GitHub" })).toBeInTheDocument();
  });

  it("退出 GitHub 会清除当前 GitHub Review Session 和界面内容", async () => {
    mockSystemTheme(false);
    invoke.mockImplementation((command: string) => {
      if (command === "github_auth_status") return Promise.resolve({
        configured: true, signedIn: true, appInstallUrl: "https://github.com/apps/narrant/installations/new",
      });
      if (command === "open_github_pull_request") return Promise.resolve({
        sessionId: "private-session", source: "github", title: "Private", comparisonBase: "base",
        headRevision: "head", fileChanges: [],
      });
      return Promise.resolve();
    });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "连接 GitHub" }));
    fireEvent.change(screen.getByLabelText("GitHub Pull Request URL"), {
      target: { value: "https://github.com/acme/private/pull/1" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "打开" }));
    expect(await screen.findByRole("heading", { name: "Private" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "退出 GitHub" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("sign_out_github"));
    expect(invoke).toHaveBeenCalledWith("close_review_session", { sessionId: "private-session" });
    expect(screen.queryByRole("heading", { name: "Private" })).not.toBeInTheDocument();
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
      sessionId: "session-github-8",
      source: "github",
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

    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("read_file_change", { sessionId: "session-github-8", path: "README.md" }));
    expect(await screen.findByTestId("diff-editor")).toBeInTheDocument();
  });

  it("公开 Pull Request 没有 File Change 时显示简体中文空状态", async () => {
    mockSystemTheme(false);
    invoke.mockResolvedValueOnce({
      sessionId: "session-github-9",
      source: "github",
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

  it("打开新的 Review Snapshot 时关闭之前的 Review Session", async () => {
    mockSystemTheme(false);
    const snapshots = ["session-1", "session-2"].map((sessionId) => ({
      sessionId, source: "github",
      owner: "octo-org", repository: "octo-repo", title: sessionId, number: 1,
      author: "octocat", state: "open", draft: false, baseRef: "main",
      baseRevision: "base", comparisonBase: "base", headRef: "topic",
      headRevision: "head", description: "", changedFiles: 0, additions: 0,
      deletions: 0, githubUrl: "https://github.com/octo-org/octo-repo/pull/1",
      fileChanges: [],
    }));
    invoke.mockImplementation((command: string) => {
      if (command === "open_github_pull_request") return Promise.resolve(snapshots.shift());
      return Promise.resolve();
    });
    render(<App />);

    fireEvent.change(screen.getByLabelText("GitHub Pull Request URL"), { target: { value: "https://github.com/octo-org/octo-repo/pull/1" } });
    fireEvent.click(screen.getByRole("button", { name: "匿名打开" }));
    expect(await screen.findByRole("heading", { name: "session-1" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "匿名打开" }));

    expect(await screen.findByRole("heading", { name: "session-2" })).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("close_review_session", { sessionId: "session-1" });
  });

  it("卸载应用时关闭活动 Review Session", async () => {
    mockSystemTheme(false);
    invoke.mockResolvedValue({
      sessionId: "session-unmount", source: "github", title: "Unmount", comparisonBase: "base",
      headRevision: "head", fileChanges: [],
    });
    const view = render(<App />);

    fireEvent.change(screen.getByLabelText("GitHub Pull Request URL"), { target: { value: "https://github.com/octo-org/octo-repo/pull/1" } });
    fireEvent.click(screen.getByRole("button", { name: "匿名打开" }));
    expect(await screen.findByRole("heading", { name: "Unmount" })).toBeInTheDocument();
    view.unmount();

    expect(invoke).toHaveBeenCalledWith("close_review_session", { sessionId: "session-unmount" });
  });

  it("读取新的本地仓库失败时关闭之前的 Review Session", async () => {
    mockSystemTheme(false);
    invoke.mockImplementation((command: string) => {
      if (command === "open_github_pull_request") {
        return Promise.resolve({
          sessionId: "session-before-load", source: "github", title: "Existing",
          comparisonBase: "base", headRevision: "head", fileChanges: [],
        });
      }
      if (command === "list_local_refs") return Promise.reject("仓库不可用");
      return Promise.resolve();
    });
    render(<App />);

    fireEvent.change(screen.getByLabelText("GitHub Pull Request URL"), { target: { value: "https://github.com/octo-org/octo-repo/pull/1" } });
    fireEvent.click(screen.getByRole("button", { name: "匿名打开" }));
    expect(await screen.findByRole("heading", { name: "Existing" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("本地仓库路径"), { target: { value: "/missing" } });
    fireEvent.click(screen.getByRole("button", { name: "读取分支" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("仓库不可用");
    expect(invoke).toHaveBeenCalledWith("close_review_session", { sessionId: "session-before-load" });
  });

  it("展示完整文件树并按需加载所选 File Change", async () => {
    mockSystemTheme(false);
    invoke
      .mockResolvedValueOnce(["base", "origin/topic"])
      .mockResolvedValueOnce({
        reviewId: "review-1",
        source: "local",
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

    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("read_review_file_change", {
      reviewId: "review-1",
      path: "assets/logo.bin",
      operationId: expect.any(String),
    }));
    expect(await screen.findByText("二进制文件（变更前：无；变更后：128 字节）")).toBeInTheDocument();
  });

  it("检查到分支移动后保留当前 Snapshot，直到用户明确刷新", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 24, canonicalPath: "/repo", commonDirectory: "/repo/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    const oldSnapshot = {
      reviewId: "review-24", source: "local", baseRef: "refs/heads/main",
      baseRevision: "base-old", comparisonBase: "base-old",
      headRef: "refs/heads/topic", headRevision: "head-old",
      fileChanges: [{ path: "old.txt", oldPath: null, status: "modified" }],
    };
    const newSnapshot = {
      ...oldSnapshot, baseRevision: "base-new", comparisonBase: "base-new",
      headRevision: "head-new",
      fileChanges: [{ path: "new.txt", oldPath: null, status: "added" }],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "list_repository_entries") return Promise.resolve([entry]);
      if (command === "list_reviews") return Promise.resolve([]);
      if (command === "open_repository_entry") return Promise.resolve(entry);
      if (command === "repository_workspace_trust") return Promise.resolve(true);
      if (command === "discover_github_pull_requests") return Promise.resolve([]);
      if (command === "list_local_refs") {
        return Promise.resolve(["refs/heads/main", "refs/heads/topic"]);
      }
      if (command === "compare_local_refs") return Promise.resolve(oldSnapshot);
      if (command === "check_branch_review_updates") return Promise.resolve({
        changed: true,
        currentBaseRevision: "base-old", currentHeadRevision: "head-old",
        latestBaseRevision: "base-new", latestHeadRevision: "head-new",
      });
      if (command === "refresh_branch_review") return Promise.resolve(newSnapshot);
      return Promise.resolve();
    });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "/repo" }));
    await screen.findByRole("heading", { name: "repo" });
    fireEvent.click(screen.getByRole("button", { name: "读取分支" }));
    await waitFor(() => expect(screen.getByLabelText("Head Ref")).toHaveTextContent("refs/heads/topic"));
    fireEvent.click(screen.getByRole("button", { name: "打开 Review Snapshot" }));
    expect(await screen.findByRole("treeitem", { name: /old.txt/ })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "检查更新" }));

    expect(await screen.findByText(/发现新版本.*head-new/)).toBeInTheDocument();
    expect(screen.getByRole("treeitem", { name: /old.txt/ })).toBeInTheDocument();
    expect(screen.queryByRole("treeitem", { name: /new.txt/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "确认刷新到新版本" }));

    expect(await screen.findByRole("treeitem", { name: /new.txt/ })).toBeInTheDocument();
    expect(screen.queryByRole("treeitem", { name: /old.txt/ })).not.toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("refresh_branch_review", {
      reviewId: "review-24",
      operationId: expect.any(String),
      confirmation: {
        changed: true,
        currentBaseRevision: "base-old",
        currentHeadRevision: "head-old",
        latestBaseRevision: "base-new",
        latestHeadRevision: "head-new",
      },
    });
  });

  it("Monaco Diff Editor 跟随运行时系统主题", async () => {
    const changeSystemTheme = mockSystemTheme(false);
    invoke
      .mockResolvedValueOnce(["base", "head"])
      .mockResolvedValueOnce({
        reviewId: "review-2",
        source: "local",
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

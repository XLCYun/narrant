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
import { i18n } from "@/lib/i18n";

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
  it("未信任仓库取消时留在首页，确认后才进入仓库页面", async () => {
    await i18n.changeLanguage("zh-CN");
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

    expect(await screen.findByRole("dialog", { name: "信任这个仓库？" })).toHaveTextContent(
      /checkout.*hooks.*filters.*Git LFS/i,
    );
    expect(screen.getByLabelText("Repository Catalog")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("dialog", { name: "信任这个仓库？" })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Repository Catalog")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "/work/narrant" }));
    fireEvent.click(await screen.findByRole("button", { name: "信任并打开" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      "grant_repository_workspace_trust",
      { repositoryEntryId: 21 },
    ));
    expect(await screen.findByRole("heading", { name: "narrant" })).toBeInTheDocument();
    expect(screen.getByText("/work/narrant")).toBeInTheDocument();
    expect(screen.queryByLabelText("Repository Catalog")).not.toBeInTheDocument();
    expect(screen.queryByText("已信任")).not.toBeInTheDocument();
  });

  it("从最近审核触发信任后继续打开原 Review", async () => {
    await i18n.changeLanguage("zh-CN");
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 31, canonicalPath: "/work/narrant", commonDirectory: "/work/narrant/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    const review = {
      reviewId: "review-31", repositoryEntryId: 31, repositoryPath: "/work/narrant",
      baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: 2,
    };
    invoke.mockImplementation((command: string) => {
      if (command === "list_repository_entries") return Promise.resolve([entry]);
      if (command === "list_reviews") return Promise.resolve([review]);
      if (command === "open_repository_entry") return Promise.resolve(entry);
      if (command === "repository_workspace_trust") return Promise.resolve(false);
      if (command === "reopen_review") return Promise.resolve({
        reviewId: review.reviewId, snapshotId: "snapshot-31", baseRef: review.baseRef,
        baseRevision: "base", comparisonBase: "base", headRef: review.headRef,
        headRevision: "head", fileChanges: [],
      });
      return Promise.resolve([]);
    });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /narrant · main → topic/ }));
    fireEvent.click(await screen.findByRole("button", { name: "信任并打开" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("reopen_review", {
      reviewId: "review-31",
    }));
    expect(await screen.findByText("没有 File Change")).toBeInTheDocument();
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
      reviewId: "review-23", baseRef: summary.baseRef,
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

    fireEvent.click(screen.getByRole("button", { name: "关闭 Review" }));
    fireEvent.click(await screen.findByRole("tab", { name: "审核历史" }));
    expect(screen.getByText(/refs\/heads\/main → refs\/heads\/topic/)).toBeInTheDocument();
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

  it("选择发现的 Pull Request 会通过 durable reviewId 打开本地 Review", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 25, canonicalPath: "/work/narrant", commonDirectory: "/work/narrant/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    const repository = { owner: "octo-org", name: "octo-repo" };
    const pullRequest = {
      number: 42, title: "Local PR Review", description: "Details", author: "octocat",
      state: "open", draft: true, url: "https://github.com/octo-org/octo-repo/pull/42",
      base: { ref: "main", sha: "base-sha" }, head: { ref: "feature", sha: "head-sha" },
    };
    invoke.mockImplementation((command: string) => {
      if (command === "list_repository_entries") return Promise.resolve([entry]);
      if (command === "open_repository_entry") return Promise.resolve(entry);
      if (command === "repository_workspace_trust") return Promise.resolve(true);
      if (command === "discover_github_pull_requests") return Promise.resolve([{
        repository, remoteNames: ["origin"], pullRequests: [pullRequest], error: null,
      }]);
      if (command === "open_pull_request_review") return Promise.resolve({
        reviewId: "review-42", baseRef: "main", baseRevision: "base-sha",
        comparisonBase: "base-sha", headRef: "feature", headRevision: "head-sha",
        fileChanges: [{ path: "README.md", oldPath: null, status: "modified" }],
        pullRequest: {
          owner: repository.owner, repository: repository.name, number: 42,
          title: pullRequest.title, description: pullRequest.description,
          author: pullRequest.author, state: pullRequest.state, draft: pullRequest.draft,
          url: pullRequest.url, baseRef: "main", headRef: "feature", synchronizedAt: 1,
        },
      });
      return Promise.resolve([]);
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "/work/narrant" }));
    fireEvent.click(await screen.findByRole("button", { name: /#42 Local PR Review/ }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("open_pull_request_review", {
      repositoryEntryId: 25, repository, pullRequest,
    }));
    expect(await screen.findByRole("heading", { name: "Local PR Review" })).toBeInTheDocument();
    expect(screen.getByText("#42 · octocat · 草稿 · 开放")).toBeInTheDocument();
  });

  it("检查更新不切换 Snapshot，只有显式刷新才替换 Review", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 25, canonicalPath: "/work/narrant", commonDirectory: "/work/narrant/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    const repository = { owner: "octo-org", name: "octo-repo" };
    const pullRequest = {
      number: 42, title: "Refreshable", description: "", author: "octocat",
      state: "open", draft: false, url: "https://github.com/octo-org/octo-repo/pull/42",
      base: { ref: "main", sha: "base-1" }, head: { ref: "feature", sha: "head-1" },
    };
    const snapshot = (headRevision: string) => ({
      reviewId: "review-42", baseRef: "main", baseRevision: "base-1",
      comparisonBase: "base-1", headRef: "feature", headRevision, fileChanges: [],
      pullRequest: {
        owner: repository.owner, repository: repository.name, number: 42,
        title: pullRequest.title, description: "", author: "octocat", state: "open",
        draft: false, url: pullRequest.url, baseRef: "main", headRef: "feature",
        synchronizedAt: 1,
      },
    });
    invoke.mockImplementation((command: string) => {
      if (command === "list_repository_entries") return Promise.resolve([entry]);
      if (command === "open_repository_entry") return Promise.resolve(entry);
      if (command === "repository_workspace_trust") return Promise.resolve(true);
      if (command === "discover_github_pull_requests") return Promise.resolve([{
        repository, remoteNames: ["origin"], pullRequests: [{
          ...pullRequest, head: { ref: "feature", sha: "head-2" },
        }], error: null,
      }]);
      if (command === "open_pull_request_review") return Promise.resolve(snapshot("head-1"));
      if (command === "check_pull_request_review_update") return Promise.resolve({
        updateAvailable: true, currentBaseRevision: "base-1", currentHeadRevision: "head-1",
        availableBaseRevision: "base-1", availableHeadRevision: "head-2",
      });
      if (command === "refresh_pull_request_review") return Promise.resolve(snapshot("head-2"));
      return Promise.resolve([]);
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "/work/narrant" }));
    fireEvent.click(await screen.findByRole("button", { name: /#42 Refreshable/ }));
    expect(await screen.findByText("main @ base-1 → feature @ head-1")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "检查 Pull Request 更新" }));
    expect(await screen.findByText("发现新的 Pull Request revisions；当前 Snapshot 保持不变。")).toBeInTheDocument();
    expect(screen.getByText("main @ base-1 → feature @ head-1")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "刷新 Review" }));
    expect(await screen.findByText("main @ base-1 → feature @ head-2")).toBeInTheDocument();
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

  it("展示完整文件树并按需加载所选 File Change", async () => {
    mockSystemTheme(false);
    invoke
      .mockResolvedValueOnce(["base", "origin/topic"])
      .mockResolvedValueOnce({
        reviewId: "review-1",
        snapshotId: "snapshot-1",
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
      snapshotId: "snapshot-1",
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
      reviewId: "review-24", baseRef: "refs/heads/main",
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
    fireEvent.click(screen.getByRole("tab", { name: "本地分支" }));
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

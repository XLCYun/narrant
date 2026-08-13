import { afterEach, beforeEach, describe, it, expect, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@monaco-editor/react", () => ({
  DiffEditor: ({ theme, options }: { theme?: string; options?: { renderSideBySide?: boolean; fontFamily?: string } }) => (
    <div
      data-testid="diff-editor"
      data-theme={theme}
      data-layout={options?.renderSideBySide ? "side-by-side" : "inline"}
      data-font-family={options?.fontFamily}
    />
  ),
}));
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const openUrl = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));

import { App } from "@/App";
import { i18n } from "@/lib/i18n";
import type { PullRequestMetadata, RecentReviewSummary, RepositoryReviewSummary } from "@/lib/review-types";

/** 用紧凑输入构造 Repository Entry 范围内的 Review 测试投影。 */
function repositoryReview(input: {
  reviewId: string; baseRef: string; headRef: string; lastOpenedAt: number;
  baseRevision?: string; comparisonBase?: string; headRevision?: string; pullRequest?: PullRequestMetadata;
}): RepositoryReviewSummary {
  return {
    reviewId: input.reviewId,
    selection: input.pullRequest
      ? { kind: "pull-request", baseRef: input.baseRef, headRef: input.headRef, pullRequest: input.pullRequest }
      : { kind: "branch", baseRef: input.baseRef, headRef: input.headRef },
    currentSnapshot: {
      baseRevision: input.baseRevision ?? "base",
      comparisonBase: input.comparisonBase ?? "merge-base",
      headRevision: input.headRevision ?? "head",
    },
    lastOpenedAt: input.lastOpenedAt,
  };
}

/** 为首页 Catalog 包装 Review 的 Repository 所有权。 */
function recentReview(repositoryEntryId: number, repositoryPath: string, review: RepositoryReviewSummary): RecentReviewSummary {
  return { repositoryEntryId, repositoryPath, review };
}

beforeEach(async () => {
  await i18n.changeLanguage("zh-CN");
});

afterEach(() => {
  cleanup();
  invoke.mockReset();
  openUrl.mockReset();
  localStorage.clear();
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 });
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

/** 从默认审核概览显式切换到文件改动。 */
async function openFileChanges() {
  fireEvent.click(await screen.findByRole("tab", { name: "文件改动" }));
}

describe("App", () => {
  it("打开 Review Snapshot 时在入口显示 Loading 图标并保留来源页面", async () => {
    mockSystemTheme(false);
    const paintFrames: FrameRequestCallback[] = [];
    const animationFrame = vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      paintFrames.push(callback);
      return paintFrames.length;
    });
    let resolveComparison!: (value: unknown) => void;
    const comparison = new Promise((resolve) => { resolveComparison = resolve; });
    invoke.mockImplementation((command: string) => {
      if (command === "list_local_refs") {
        return Promise.resolve({ refs: ["base", "head"], defaultRef: "base", currentRef: "head" });
      }
      if (command === "compare_local_refs") return comparison;
      return Promise.resolve([]);
    });
    render(<App />);

    fireEvent.change(screen.getByLabelText("本地仓库路径"), { target: { value: "/repo" } });
    fireEvent.click(screen.getByRole("button", { name: "重新读取分支" }));
    await waitFor(() => expect(screen.getByLabelText("Head Ref")).toHaveTextContent("head"));
    const openButton = screen.getByRole("button", { name: "开始审核" });
    fireEvent.click(openButton);

    expect(openButton).toHaveAttribute("aria-busy", "true");
    expect(openButton.querySelector(".animate-spin")).toBeInTheDocument();
    expect(screen.getByLabelText("选择本地仓库")).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("compare_local_refs", expect.anything());
    const firstPaintFrames = paintFrames.splice(0);
    await act(async () => { firstPaintFrames.forEach((frame) => frame(0)); });
    await waitFor(() => expect(paintFrames.length).toBeGreaterThan(0));
    expect(invoke).not.toHaveBeenCalledWith("compare_local_refs", expect.anything());
    const secondPaintFrames = paintFrames.splice(0);
    await act(async () => { secondPaintFrames.forEach((frame) => frame(16)); });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("compare_local_refs", expect.anything()));
    resolveComparison({
      reviewId: "review-loading", snapshotId: "snapshot-loading", baseRef: "base",
      baseRevision: "base-sha", comparisonBase: "base-sha", headRef: "head",
      headRevision: "head-sha", fileChanges: [],
    });
    await openFileChanges();
    expect(await screen.findByText("没有 File Change")).toBeInTheDocument();
    animationFrame.mockRestore();
  });

  it("读取 File Change 时在 Diff 区域同时显示 Spinner 与 Skeleton", async () => {
    mockSystemTheme(false);
    let resolveFile!: (value: unknown) => void;
    const file = new Promise((resolve) => { resolveFile = resolve; });
    invoke.mockImplementation((command: string) => {
      if (command === "list_local_refs") {
        return Promise.resolve({ refs: ["base", "head"], defaultRef: "base", currentRef: "head" });
      }
      if (command === "compare_local_refs") return Promise.resolve({
        reviewId: "review-file-loading", snapshotId: "snapshot-file-loading", baseRef: "base",
        baseRevision: "base-sha", comparisonBase: "base-sha", headRef: "head",
        headRevision: "head-sha", fileChanges: [{ path: "slow.txt", oldPath: null, status: "modified" }],
      });
      if (command === "read_review_file_change") return file;
      return Promise.resolve([]);
    });
    render(<App />);

    fireEvent.change(screen.getByLabelText("本地仓库路径"), { target: { value: "/repo" } });
    fireEvent.click(screen.getByRole("button", { name: "重新读取分支" }));
    await waitFor(() => expect(screen.getByLabelText("Head Ref")).toHaveTextContent("head"));
    fireEvent.click(screen.getByRole("button", { name: "开始审核" }));

    await openFileChanges();
    const loading = await screen.findByRole("status", { name: "正在读取 File Change" });
    expect(loading).toHaveAttribute("aria-busy", "true");
    expect(loading.querySelector(".animate-spin")).toBeInTheDocument();
    expect(loading.querySelector("[data-slot=skeleton]")).toBeInTheDocument();
    resolveFile({ path: "slow.txt", oldPath: null, status: "modified", contentKind: "text", beforeSize: 3, afterSize: 3, before: "old", after: "new" });
    expect(await screen.findByRole("heading", { name: /slow.txt/ })).toBeInTheDocument();
  });

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
      if (command === "load_repository_catalog") return Promise.resolve({ repositories: [], recentReviews: [] });
      if (command === "register_repository_entry") return Promise.resolve(entry);
      if (command === "open_repository_overview") return Promise.resolve({ kind: "trust-required", repository: entry });
      if (command === "grant_repository_workspace_trust_and_open") return Promise.resolve({ repository: entry, reviews: [] });
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
      "grant_repository_workspace_trust_and_open",
      { repositoryEntryId: 21 },
    ));
    expect(await screen.findByRole("heading", { name: "narrant" })).toBeInTheDocument();
    expect(screen.queryByText("/work/narrant")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Repository Catalog")).not.toBeInTheDocument();
    expect(screen.queryByText("已信任")).not.toBeInTheDocument();
    expect(screen.getByLabelText("主侧边栏")).toBeInTheDocument();
  });

  it("从最近审核触发信任后继续打开原 Review", async () => {
    await i18n.changeLanguage("zh-CN");
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 31, canonicalPath: "/work/narrant", commonDirectory: "/work/narrant/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    const review = repositoryReview({ reviewId: "review-31", baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: 2 });
    const recent = recentReview(31, "/work/narrant", review);
    invoke.mockImplementation((command: string) => {
      if (command === "load_repository_catalog") return Promise.resolve({ repositories: [entry], recentReviews: [recent] });
      if (command === "open_repository_overview") return Promise.resolve({ kind: "trust-required", repository: entry });
      if (command === "grant_repository_workspace_trust_and_open") return Promise.resolve({ repository: entry, reviews: [review] });
      if (command === "reopen_review") return Promise.resolve({
        reviewId: review.reviewId, snapshotId: "snapshot-31", baseRef: review.selection.baseRef,
        baseRevision: "base", comparisonBase: "base", headRef: review.selection.headRef,
        headRevision: "head", fileChanges: [],
      });
      return Promise.resolve([]);
    });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /将 Head 分支 topic 合入 Base 分支 main/ }));
    fireEvent.click(await screen.findByRole("button", { name: "信任并打开" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("reopen_review", {
      reviewId: "review-31",
    }));
    await openFileChanges();
    expect(await screen.findByText("没有 File Change")).toBeInTheDocument();
  });

  it("从首页最近列表和 Repository Entry 重新打开持久化 Review", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 23, canonicalPath: "/work/narrant", commonDirectory: "/work/narrant/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    const summary = repositoryReview({ reviewId: "review-23", baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: 20 });
    const newerSummary = repositoryReview({ reviewId: "review-24", baseRef: "refs/heads/release", headRef: "refs/heads/hotfix", lastOpenedAt: 30 });
    const snapshot = {
      reviewId: "review-23", baseRef: summary.selection.baseRef,
      baseRevision: "base", comparisonBase: "base", headRef: summary.selection.headRef,
      headRevision: "head", fileChanges: [],
    };
    invoke.mockImplementation((command: string) => {
      if (command === "load_repository_catalog") return Promise.resolve({ repositories: [entry], recentReviews: [recentReview(23, "/work/narrant", newerSummary), recentReview(23, "/work/narrant", summary)] });
      if (command === "open_repository_overview") return Promise.resolve({
        kind: "ready", overview: { repository: entry, reviews: [summary] },
      });
      if (command === "load_repository_reviews") return Promise.resolve({ reviews: [summary] });
      if (command === "reopen_review") return Promise.resolve(snapshot);
      return Promise.resolve([]);
    });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /将 Head 分支 topic 合入 Base 分支 main/ }));
    await openFileChanges();
    expect(await screen.findByText("没有 File Change")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("reopen_review", { reviewId: "review-23" });

    fireEvent.click(screen.getByRole("button", { name: "退出审核" }));
    expect(await screen.findByRole("tab", { name: "审核历史", selected: true })).toBeInTheDocument();
    expect(screen.getAllByLabelText("将 Head 分支 topic 合入 Base 分支 main").length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: "纳言" }));
    const recentButtons = screen.getByRole("region", { name: "Recent Reviews" })
      .querySelectorAll("button");
    expect(recentButtons[0]).toHaveAccessibleName(/将 Head 分支 topic 合入 Base 分支 main/);
  });

  it("扁平展示多个 GitHub repository 的 Pull Requests，且保留局部失败", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 25, canonicalPath: "/work/narrant", commonDirectory: "/work/narrant/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    invoke.mockImplementation((command: string) => {
      if (command === "load_repository_catalog") return Promise.resolve({ repositories: [entry], recentReviews: [] });
      if (command === "open_repository_overview") return Promise.resolve({
        kind: "ready", overview: { repository: entry, reviews: [] },
      });
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
            updatedAt: "2026-07-31T08:00:00Z",
            base: { ref: "main", sha: "base-sha" },
            head: { ref: "feature", sha: "head-sha" },
          }],
        },
      ]);
      return Promise.resolve([]);
    });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "/work/narrant" }));

    expect(await screen.findByText("#42 Metadata only")).toBeInTheDocument();
    expect(screen.getByText("octo-org/octo-repo")).toBeInTheDocument();
    expect(screen.queryByText(/remotes:/)).not.toBeInTheDocument();
    expect(screen.getByText(/fork-owner\/fork-repo/)).toHaveTextContent(/GitHub 访问次数已用尽/);
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
      updatedAt: "2026-07-31T08:00:00Z",
      base: { ref: "main", sha: "base-sha" }, head: { ref: "feature", sha: "head-sha" },
    };
    invoke.mockImplementation((command: string) => {
      if (command === "load_repository_catalog") return Promise.resolve({ repositories: [entry], recentReviews: [] });
      if (command === "open_repository_overview") return Promise.resolve({
        kind: "ready", overview: { repository: entry, reviews: [] },
      });
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
    fireEvent.click(await screen.findByRole("button", { name: /开始审核 #42 Local PR Review/ }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("open_pull_request_review", {
      operationId: expect.any(String), repositoryEntryId: 25, repository, number: 42,
    }));
    expect(await screen.findByRole("heading", { name: "Local PR Review" })).toBeInTheDocument();
    expect(screen.getByLabelText("审核阅读器")).toHaveTextContent("草稿");
    expect(screen.getByRole("tabpanel", { name: "审核概览" })).toHaveTextContent(/草稿.*#42.*octocat/s);
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
      updatedAt: "2026-07-31T08:00:00Z",
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
      if (command === "load_repository_catalog") return Promise.resolve({ repositories: [entry], recentReviews: [] });
      if (command === "open_repository_overview") return Promise.resolve({
        kind: "ready", overview: { repository: entry, reviews: [] },
      });
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
    fireEvent.click(await screen.findByRole("button", { name: /开始审核 #42 Refreshable/ }));
    expect(await screen.findByText("检测到 PR 分支有更新")).toBeInTheDocument();
    expect(screen.getByLabelText("审核阅读器")).not.toHaveTextContent("base-1 → head-1");
    fireEvent.click(screen.getByRole("button", { name: "刷新审核" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("refresh_pull_request_review", {
      operationId: expect.any(String),
      reviewId: "review-42",
      confirmation: {
        currentBaseRevision: "base-1",
        currentHeadRevision: "head-1",
        availableBaseRevision: "base-1",
        availableHeadRevision: "head-2",
      },
    }));
    expect(screen.getByRole("tabpanel", { name: "审核概览" })).toHaveTextContent("head-2");
  });

  it("切换 Review 失败时保留当前 Snapshot 与 File Change", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 25, canonicalPath: "/work/narrant", commonDirectory: "/work/narrant/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    const current = repositoryReview({ reviewId: "review-current", baseRef: "refs/heads/main", headRef: "refs/heads/current", lastOpenedAt: 2 });
    const target = repositoryReview({ reviewId: "review-target", baseRef: "refs/heads/main", headRef: "refs/heads/target", lastOpenedAt: 1 });
    invoke.mockImplementation((command: string, arguments_?: Record<string, unknown>) => {
      if (command === "load_repository_catalog") return Promise.resolve({ repositories: [entry], recentReviews: [recentReview(25, "/work/narrant", current), recentReview(25, "/work/narrant", target)] });
      if (command === "open_repository_overview") return Promise.resolve({
        kind: "ready", overview: { repository: entry, reviews: [current, target] },
      });
      if (command === "load_repository_reviews") return Promise.resolve({ reviews: [current, target] });
      if (command === "reopen_review" && arguments_?.reviewId === current.reviewId) {
        return Promise.resolve({
          reviewId: current.reviewId, snapshotId: "snapshot-current", baseRef: current.selection.baseRef,
          baseRevision: "base", comparisonBase: "base", headRef: current.selection.headRef,
          headRevision: "head", fileChanges: [{ path: "kept.txt", oldPath: null, status: "modified" }],
        });
      }
      if (command === "read_review_file_change") {
        return Promise.resolve({ path: "kept.txt", oldPath: null, status: "modified", contentKind: "text", beforeSize: 3, afterSize: 3, before: "old", after: "new" });
      }
      if (command === "reopen_review" && arguments_?.reviewId === target.reviewId) {
        return Promise.reject(new Error("目标审核打开失败"));
      }
      return Promise.resolve([]);
    });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /将 Head 分支 current 合入 Base 分支 main/ }));
    await openFileChanges();
    expect(await screen.findByRole("button", { name: /kept.txt/ })).toBeInTheDocument();

    const targetReview = await waitFor(() => {
      const item = document.querySelector<HTMLButtonElement>('[data-review-id="review-target"]');
      expect(item).toBeInTheDocument();
      return item!;
    });
    fireEvent.click(targetReview);

    expect(await screen.findByRole("alert")).toHaveTextContent("目标审核打开失败");
    expect(screen.getByRole("button", { name: /kept.txt/ })).toBeInTheDocument();
  });

  it("保留并展示已移动 Repository Entry 的可恢复不可用状态", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockImplementation((command: string) => {
      if (command === "load_repository_catalog") return Promise.resolve({ repositories: [{
        id: 22,
        canonicalPath: "/missing/repository",
        commonDirectory: "/missing/repository/.git",
        createdAt: 1,
        lastOpenedAt: 1,
        available: false,
        unavailableReason: "Repository Entry 路径不存在或不可访问",
      }], recentReviews: [] });
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
    expect(screen.getByRole("heading", { name: "纳言" })).toBeInTheDocument();
    expect(screen.getByText("本地分支比较")).toBeInTheDocument();
  });

  it("仓库页面默认选择默认与当前分支，并识别可继续的 Branch Selection", async () => {
    await i18n.changeLanguage("zh-CN");
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 41, canonicalPath: "/repo", commonDirectory: "/repo/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    const existingReview = repositoryReview({ reviewId: "review-41", baseRef: "refs/remotes/origin/main", headRef: "refs/heads/topic", lastOpenedAt: 2 });
    invoke.mockImplementation((command: string) => {
      if (command === "load_repository_catalog") return Promise.resolve({ repositories: [entry], recentReviews: [recentReview(41, "/repo", existingReview)] });
      if (command === "open_repository_overview") return Promise.resolve({
        kind: "ready", overview: { repository: entry, reviews: [existingReview] },
      });
      if (command === "load_repository_reviews") return Promise.resolve({ reviews: [existingReview] });
      if (command === "discover_github_pull_requests") return Promise.resolve([]);
      if (command === "list_local_refs") return Promise.resolve({
        refs: ["refs/heads/topic", "refs/remotes/origin/main"],
        defaultRef: "refs/remotes/origin/main",
        currentRef: "refs/heads/topic",
      });
      if (command === "check_branch_review_updates") return Promise.resolve({
        changed: false,
        currentBaseRevision: "base-1",
        currentHeadRevision: "head-1",
        latestBaseRevision: "base-1",
        latestHeadRevision: "head-1",
      });
      return Promise.resolve();
    });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "/repo" }));
    fireEvent.click(await screen.findByRole("tab", { name: "本地分支" }));
    fireEvent.click(screen.getByRole("button", { name: "重新读取分支" }));

    await waitFor(() => expect(screen.getByLabelText("Head 分支")).toHaveTextContent("topic"));
    expect(screen.getByLabelText("Base 分支")).toHaveTextContent("origin/main");
    expect(screen.getByText("将 topic 合入 origin/main")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "继续审核" })).toBeEnabled();
    expect(invoke).toHaveBeenCalledWith("check_branch_review_updates", {
      operationId: expect.any(String),
      reviewId: "review-41",
    });
  });

  it("已有 Branch Selection 更新后创建最新 Snapshot 再继续审核", async () => {
    mockSystemTheme(false);
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const entry = {
      id: 42, canonicalPath: "/repo", commonDirectory: "/repo/.git",
      createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null,
    };
    const review = repositoryReview({ reviewId: "review-42", baseRef: "refs/remotes/origin/main", headRef: "refs/heads/topic", lastOpenedAt: 2 });
    const update = {
      changed: true,
      currentBaseRevision: "base-1",
      currentHeadRevision: "head-1",
      latestBaseRevision: "base-2",
      latestHeadRevision: "head-2",
    };
    const snapshot = (snapshotId: string, baseRevision: string, headRevision: string) => ({
      reviewId: review.reviewId,
      snapshotId,
      baseRef: review.selection.baseRef,
      baseRevision,
      comparisonBase: baseRevision,
      headRef: review.selection.headRef,
      headRevision,
      fileChanges: [],
    });
    invoke.mockImplementation((command: string) => {
      if (command === "load_repository_catalog") return Promise.resolve({ repositories: [entry], recentReviews: [recentReview(42, "/repo", review)] });
      if (command === "open_repository_overview") return Promise.resolve({
        kind: "ready", overview: { repository: entry, reviews: [review] },
      });
      if (command === "load_repository_reviews") return Promise.resolve({ reviews: [review] });
      if (command === "discover_github_pull_requests") return Promise.resolve([]);
      if (command === "list_local_refs") return Promise.resolve({
        refs: [review.selection.headRef, review.selection.baseRef], defaultRef: review.selection.baseRef, currentRef: review.selection.headRef,
      });
      if (command === "check_branch_review_updates") return Promise.resolve(update);
      if (command === "reopen_review") return Promise.resolve(snapshot("snapshot-1", "base-1", "head-1"));
      if (command === "reopen_and_refresh_branch_review") return Promise.resolve(snapshot("snapshot-2", "base-2", "head-2"));
      return Promise.resolve();
    });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "/repo" }));
    fireEvent.click(await screen.findByRole("tab", { name: "本地分支" }));
    fireEvent.click(screen.getByRole("button", { name: "重新读取分支" }));
    fireEvent.click(await screen.findByRole("button", { name: "更新并继续审核" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("reopen_and_refresh_branch_review", {
      operationId: expect.any(String),
      reviewId: "review-42",
      confirmation: update,
    }));
    expect(screen.getByRole("tabpanel", { name: "审核概览" })).toHaveTextContent(/head-2.*base-2/s);
  });

  it("Base 与 Head 相同时禁用开始审核并显示可翻译说明", async () => {
    await i18n.changeLanguage("en");
    mockSystemTheme(false);
    invoke.mockResolvedValue({
      refs: ["refs/heads/main"],
      defaultRef: "refs/heads/main",
      currentRef: "refs/heads/main",
    });
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "重新读取分支" }));

    expect(await screen.findByRole("button", { name: "Start review" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Head and Base must be different branches.");
    await i18n.changeLanguage("zh-CN");
  });

  it("展示完整文件树并按需加载所选 File Change", async () => {
    mockSystemTheme(false);
    invoke
      .mockResolvedValueOnce({
        refs: ["base", "origin/topic"],
        defaultRef: "base",
        currentRef: "origin/topic",
      })
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
    fireEvent.click(screen.getByRole("button", { name: "重新读取分支" }));
    await waitFor(() => expect(screen.getByLabelText("Head Ref")).toHaveTextContent("origin/topic"));
    fireEvent.click(screen.getByRole("button", { name: "开始审核" }));

    await openFileChanges();
    expect(await screen.findByRole("navigation", { name: "文件树" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "src" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: /main.rs.*修改/ })).toBeInTheDocument();
    expect(screen.getByTestId("file-type-icon-src")).toHaveAttribute("src", expect.stringContaining("folder-src-open.svg"));
    expect(screen.getByTestId("file-type-icon-main.rs")).toHaveAttribute("src", expect.stringContaining("rust.svg"));
    expect(screen.getAllByRole("img", { name: "新增" })[0]).toHaveClass("text-emerald-600", "bg-emerald-500/10");

    fireEvent.click(screen.getByRole("button", { name: "src" }));
    expect(screen.getByRole("button", { name: "src" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByTestId("file-type-icon-src")).toHaveAttribute("src", expect.stringContaining("folder-src.svg"));
    expect(screen.queryByRole("button", { name: /main.rs.*修改/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "src" }));
    expect(screen.getByRole("button", { name: /main.rs.*修改/ })).toBeInTheDocument();

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("read_review_file_change", {
      reviewId: "review-1",
      snapshotId: "snapshot-1",
      path: "assets/logo.bin",
      operationId: expect.any(String),
    }));
    expect(await screen.findByText("二进制文件（变更前：无；变更后：128 字节）")).toBeInTheDocument();
  });

  it("Review Reader 只保留当前 Review 工具栏并把 revisions 收进概览", async () => {
    mockSystemTheme(false);
    invoke.mockImplementation((command: string) => {
      if (command === "list_local_refs") {
        return Promise.resolve({ refs: ["main", "topic"], defaultRef: "main", currentRef: "topic" });
      }
      if (command === "compare_local_refs") return Promise.resolve({
        reviewId: "review-toolbar", snapshotId: "snapshot-toolbar",
        baseRef: "main", baseRevision: "base-revision-full", comparisonBase: "base-revision-full",
        headRef: "topic", headRevision: "head-revision-full", fileChanges: [],
      });
      return Promise.resolve([]);
    });
    render(<App />);

    fireEvent.change(screen.getByLabelText("本地仓库路径"), { target: { value: "/repo" } });
    fireEvent.click(screen.getByRole("button", { name: "重新读取分支" }));
    await waitFor(() => expect(screen.getByLabelText("Head Ref")).toHaveTextContent("topic"));
    fireEvent.click(screen.getByRole("button", { name: "开始审核" }));

    const reader = await screen.findByLabelText("审核阅读器");
    expect(screen.queryByRole("navigation", { name: "切换审核" })).not.toBeInTheDocument();
    expect(reader).toHaveTextContent("base-re");
    expect(reader).toHaveTextContent("head-re");
  });

  it("打开 Review 后按稳定路径顺序自动选择首个 File Change 并高亮当前位置", async () => {
    mockSystemTheme(false);
    invoke.mockImplementation((command: string, arguments_?: Record<string, unknown>) => {
      if (command === "list_local_refs") {
        return Promise.resolve({ refs: ["base", "head"], defaultRef: "base", currentRef: "head" });
      }
      if (command === "compare_local_refs") {
        return Promise.resolve({
          reviewId: "review-auto",
          snapshotId: "snapshot-auto",
          baseRef: "base",
          baseRevision: "base-sha",
          comparisonBase: "base-sha",
          headRef: "head",
          headRevision: "head-sha",
          fileChanges: [
            { path: "z-last.txt", oldPath: null, status: "modified" },
            { path: "a-first.txt", oldPath: null, status: "added" },
          ],
        });
      }
      if (command === "read_review_file_change") {
        return Promise.resolve({
          path: arguments_?.path,
          oldPath: null,
          status: "added",
          contentKind: "text",
          beforeSize: null,
          afterSize: 3,
          before: null,
          after: "new",
        });
      }
      return Promise.resolve([]);
    });
    render(<App />);

    fireEvent.change(screen.getByLabelText("本地仓库路径"), { target: { value: "/repo" } });
    fireEvent.click(screen.getByRole("button", { name: "重新读取分支" }));
    await waitFor(() => expect(screen.getByLabelText("Head Ref")).toHaveTextContent("head"));
    fireEvent.click(screen.getByRole("button", { name: "开始审核" }));

    await openFileChanges();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("read_review_file_change", {
      reviewId: "review-auto",
      snapshotId: "snapshot-auto",
      path: "a-first.txt",
      operationId: expect.any(String),
    }));
    expect(await screen.findByRole("button", { name: /a-first.txt.*新增/ }))
      .toHaveAttribute("aria-current", "true");
    const fileChangeHeading = screen.getByRole("heading", { name: /a-first.txt/ });
    expect(fileChangeHeading).toBeInTheDocument();
    expect(fileChangeHeading.querySelector("img")).toHaveAttribute("src", expect.stringContaining("document.svg"));

    await i18n.changeLanguage("en");
    expect(screen.getByLabelText("Review reader")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Switch to inline" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "File tree" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /a-first.txt.*Added/ })).toBeInTheDocument();
  });

  it("为 Binary、Symlink 和 Submodule 显示专用信息视图而不实例化 Monaco", async () => {
    mockSystemTheme(false);
    invoke.mockImplementation((command: string, arguments_?: Record<string, unknown>) => {
      if (command === "list_local_refs") {
        return Promise.resolve({ refs: ["base", "head"], defaultRef: "base", currentRef: "head" });
      }
      if (command === "compare_local_refs") {
        return Promise.resolve({
          reviewId: "review-special",
          snapshotId: "snapshot-special",
          baseRef: "base",
          baseRevision: "base-sha",
          comparisonBase: "base-sha",
          headRef: "head",
          headRevision: "head-sha",
          fileChanges: [
            { path: "assets/image.bin", oldPath: null, status: "added" },
            { path: "deps/library", oldPath: null, status: "modified" },
            { path: "latest", oldPath: null, status: "modified" },
          ],
        });
      }
      if (command === "read_review_file_change") {
        const path = String(arguments_?.path);
        const contentKind = path.endsWith(".bin") ? "binary" : path.startsWith("deps/") ? "submodule" : "symlink";
        return Promise.resolve({
          path,
          oldPath: null,
          status: "modified",
          contentKind,
          beforeSize: contentKind === "binary" ? null : 7,
          afterSize: contentKind === "binary" ? 128 : 8,
          before: contentKind === "submodule" ? "abc1234" : contentKind === "symlink" ? "v1" : null,
          after: contentKind === "submodule" ? "def5678" : contentKind === "symlink" ? "v2" : null,
        });
      }
      return Promise.resolve([]);
    });
    render(<App />);
    fireEvent.change(screen.getByLabelText("本地仓库路径"), { target: { value: "/repo" } });
    fireEvent.click(screen.getByRole("button", { name: "重新读取分支" }));
    await waitFor(() => expect(screen.getByLabelText("Head Ref")).toHaveTextContent("head"));
    fireEvent.click(screen.getByRole("button", { name: "开始审核" }));

    await openFileChanges();
    expect(await screen.findByText(/二进制文件/)).toBeInTheDocument();
    expect(screen.queryByTestId("diff-editor")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /library.*修改/ }));
    expect(await screen.findByText("Submodule commit 变更")).toBeInTheDocument();
    expect(screen.getByText("abc1234 → def5678")).toBeInTheDocument();
    expect(screen.queryByTestId("diff-editor")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /latest.*修改/ }));
    expect(await screen.findByText("Symlink 目标变更")).toBeInTheDocument();
    expect(screen.getByText("v1 → v2")).toBeInTheDocument();
    expect(screen.queryByTestId("diff-editor")).not.toBeInTheDocument();
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
      if (command === "load_repository_catalog") return Promise.resolve({ repositories: [entry], recentReviews: [] });
      if (command === "open_repository_overview") return Promise.resolve({
        kind: "ready", overview: { repository: entry, reviews: [] },
      });
      if (command === "discover_github_pull_requests") return Promise.resolve([]);
      if (command === "list_local_refs") {
        return Promise.resolve({
          refs: ["refs/heads/main", "refs/heads/topic"],
          defaultRef: "refs/heads/main",
          currentRef: "refs/heads/topic",
        });
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
    fireEvent.click(screen.getByRole("button", { name: "重新读取分支" }));
    await waitFor(() => expect(screen.getByLabelText("Head 分支")).toHaveTextContent("topic"));
    fireEvent.click(screen.getByRole("button", { name: "开始审核" }));
    await openFileChanges();
    expect(await screen.findByRole("button", { name: /old.txt/ })).toBeInTheDocument();

    expect(await screen.findByText("检测到所选分支有更新")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /old.txt/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /new.txt/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "刷新审核" }));

    expect(await screen.findByRole("button", { name: /new.txt/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /old.txt/ })).not.toBeInTheDocument();
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
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 });
    localStorage.setItem("narrant.review.fileTreeWidth", "31");
    const changeSystemTheme = mockSystemTheme(false);
    invoke
      .mockResolvedValueOnce({
        refs: ["base", "head"],
        defaultRef: "base",
        currentRef: "head",
      })
      .mockResolvedValueOnce({
        reviewId: "review-2",
        comparisonBase: "base-sha",
        headRevision: "head-sha",
        fileChanges: [{ path: "readme.txt", oldPath: null, status: "modified", contentKind: "text", beforeSize: 3, afterSize: 4, before: null, after: null }],
      })
      .mockResolvedValueOnce({ path: "readme.txt", oldPath: null, status: "modified", contentKind: "text", beforeSize: 3, afterSize: 4, before: "old", after: "new" });
    render(<App />);

    fireEvent.change(screen.getByLabelText("本地仓库路径"), { target: { value: "/repo" } });
    fireEvent.click(screen.getByRole("button", { name: "重新读取分支" }));
    await waitFor(() => expect(screen.getByLabelText("Head Ref")).toHaveTextContent("head"));
    fireEvent.click(screen.getByRole("button", { name: "开始审核" }));

    await openFileChanges();
    expect(await screen.findByTestId("diff-editor")).toHaveAttribute("data-theme", "light");
    expect(screen.getByTestId("diff-editor")).toHaveAttribute("data-font-family", "'JetBrains Mono Variable', monospace");
    expect(screen.getByTestId("diff-editor")).toHaveAttribute("data-layout", "side-by-side");
    expect(screen.getByRole("separator", { name: "调整文件树宽度" })).toHaveAttribute("aria-valuenow", "31");
    expect(screen.getByRole("button", { name: "退出审核" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "切换为单栏" }));
    expect(screen.getByTestId("diff-editor")).toHaveAttribute("data-layout", "inline");
    expect(localStorage.getItem("narrant.review.diffMode")).toBe("inline");
    fireEvent.click(screen.getByRole("button", { name: "切换为双栏" }));
    expect(localStorage.getItem("narrant.review.diffMode")).toBe("side-by-side");

    Object.defineProperty(window, "innerWidth", { configurable: true, value: 900 });
    fireEvent(window, new Event("resize"));
    await waitFor(() => expect(screen.getByTestId("diff-editor")).toHaveAttribute("data-layout", "inline"));
    expect(screen.getByRole("button", { name: "切换为双栏" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "切换为双栏" })).toBeDisabled();
    expect(localStorage.getItem("narrant.review.diffMode")).toBe("side-by-side");

    changeSystemTheme(true);
    await waitFor(() => expect(screen.getByTestId("diff-editor")).toHaveAttribute("data-theme", "vs-dark"));
  });
});

import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { RepositoryApplicationAdapter, RepositoryEntry } from "@/repository/repository-application";
import { useRepositoryEntryOverview } from "@/repository/use-repository-entry-overview";

const firstRepository: RepositoryEntry = {
  id: 1,
  canonicalPath: "/work/first",
  commonDirectory: "/work/first/.git",
  createdAt: 1,
  lastOpenedAt: 1,
  available: true,
  unavailableReason: null,
};

const secondRepository: RepositoryEntry = {
  ...firstRepository,
  id: 2,
  canonicalPath: "/work/second",
  commonDirectory: "/work/second/.git",
};

/** 为 Repository Entry Overview seam 提供可预测的内存 adapter。 */
function createAdapter(): RepositoryApplicationAdapter {
  return {
    loadCatalog: vi.fn().mockResolvedValue({ repositories: [firstRepository, secondRepository], recentReviews: [] }),
    register: vi.fn().mockResolvedValue(firstRepository),
    open: vi.fn(async (repositoryEntryId) => ({
      kind: "ready",
      overview: {
        repository: repositoryEntryId === firstRepository.id ? firstRepository : secondRepository,
        reviews: [],
      },
    })),
    grantTrust: vi.fn(),
    loadReviews: vi.fn().mockResolvedValue({ reviews: [] }),
    loadBranches: vi.fn().mockResolvedValue({ refs: [], defaultRef: "", currentRef: "" }),
  };
}

describe("Repository Entry Overview lifecycle", () => {
  it("只在目标 Repository Entry Overview 完整就绪后原子替换当前 Overview", async () => {
    const adapter = createAdapter();
    let finishSecondOpen!: () => void;
    vi.mocked(adapter.open).mockImplementation(async (repositoryEntryId) => {
      if (repositoryEntryId === secondRepository.id) {
        await new Promise<void>((resolve) => { finishSecondOpen = resolve; });
      }
      return {
        kind: "ready",
        overview: {
          repository: repositoryEntryId === firstRepository.id ? firstRepository : secondRepository,
          reviews: [],
        },
      };
    });
    const { result } = renderHook(() => useRepositoryEntryOverview(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.catalog.status).toBe("ready"));

    await act(async () => {
      await result.current.open(firstRepository.id);
    });

    let opening!: Promise<unknown>;
    await act(async () => {
      opening = result.current.open(secondRepository.id);
      await Promise.resolve();
    });

    expect(result.current.current?.repository).toBe(firstRepository);
    expect(result.current.transition).toEqual({ kind: "opening", repositoryEntryId: 2 });

    await act(async () => {
      finishSecondOpen();
      await opening;
    });
    expect(result.current.current?.repository).toBe(secondRepository);
    expect(result.current.transition).toEqual({ kind: "stable" });
  });

  it("Workspace Trust 确认前保留旧 Overview，确认后恢复原 Repository 目标", async () => {
    const adapter = createAdapter();
    vi.mocked(adapter.open).mockImplementation(async (repositoryEntryId) => repositoryEntryId === firstRepository.id
      ? { kind: "ready", overview: { repository: firstRepository, reviews: [] } }
      : { kind: "trust-required", repository: secondRepository });
    vi.mocked(adapter.grantTrust).mockResolvedValue({
      repository: secondRepository,
      reviews: [],
    });
    const { result } = renderHook(() => useRepositoryEntryOverview(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.catalog.status).toBe("ready"));
    await act(async () => { await result.current.open(firstRepository.id); });

    await act(async () => {
      await result.current.open(secondRepository.id);
    });

    expect(result.current.current?.repository).toBe(firstRepository);
    expect(result.current.transition).toEqual({ kind: "trust-required", repository: secondRepository });

    await act(async () => {
      await result.current.confirmTrust();
    });

    expect(adapter.grantTrust).toHaveBeenCalledWith(secondRepository);
    expect(result.current.current).toEqual({
      repository: secondRepository,
      reviews: [],
    });
    expect(result.current.transition).toEqual({ kind: "stable" });
  });

  it("切换 Repository 时取消旧 Branch refs，并忽略旧请求的迟到结果", async () => {
    const adapter = createAdapter();
    let finishFirstBranches!: () => void;
    let firstSignal!: AbortSignal;
    vi.mocked(adapter.loadBranches).mockImplementation(async (repositoryEntryId, signal) => {
      if (repositoryEntryId === firstRepository.id) {
        firstSignal = signal;
        await new Promise<void>((resolve) => { finishFirstBranches = resolve; });
        return { refs: ["old"], defaultRef: "old", currentRef: "old" };
      }
      return { refs: ["main", "topic"], defaultRef: "main", currentRef: "topic" };
    });
    const { result } = renderHook(() => useRepositoryEntryOverview(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.catalog.status).toBe("ready"));
    await act(async () => { await result.current.open(firstRepository.id); });

    let oldReload!: Promise<unknown>;
    await act(async () => {
      oldReload = result.current.reloadBranches();
      await Promise.resolve();
      await result.current.open(secondRepository.id);
      await result.current.reloadBranches();
    });

    expect(firstSignal.aborted).toBe(true);
    expect(result.current.branches).toEqual({
      kind: "ready",
      value: { refs: ["main", "topic"], defaultRef: "main", currentRef: "topic" },
    });

    await act(async () => {
      finishFirstBranches();
      await oldReload;
    });
    expect(result.current.branches).toEqual({
      kind: "ready",
      value: { refs: ["main", "topic"], defaultRef: "main", currentRef: "topic" },
    });
  });

  it("记录打开的 Review 时同步当前仓库、全局最近列表和 Repository 时间", async () => {
    const adapter = createAdapter();
    const { result } = renderHook(() => useRepositoryEntryOverview(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.catalog.status).toBe("ready"));
    await act(async () => { await result.current.open(firstRepository.id); });
    const openedReview = {
      repositoryEntryId: firstRepository.id,
      repositoryPath: firstRepository.canonicalPath,
      review: {
        reviewId: "review-1",
        selection: { kind: "branch" as const, baseRef: "refs/heads/main", headRef: "refs/heads/topic" },
        currentSnapshot: { baseRevision: "base", comparisonBase: "merge-base", headRevision: "head" },
        lastOpenedAt: 42,
      },
    };

    act(() => result.current.recordReviewOpened(openedReview));

    expect(result.current.current?.reviews).toEqual([openedReview.review]);
    expect(result.current.current?.repository.lastOpenedAt).toBe(42);
    expect(result.current.catalog.recentReviews).toEqual([openedReview]);
    expect(result.current.catalog.repositories.map((entry) => entry.id)).toEqual([1, 2]);
    expect(result.current.catalog.repositories[0].lastOpenedAt).toBe(42);
  });

  it("注册新 Repository 时放到 Catalog 前端，重复注册时保持原位置", async () => {
    const adapter = createAdapter();
    const thirdRepository = { ...firstRepository, id: 3, canonicalPath: "/work/third" };
    vi.mocked(adapter.register)
      .mockResolvedValueOnce(thirdRepository)
      .mockResolvedValueOnce({ ...secondRepository, available: false });
    const { result } = renderHook(() => useRepositoryEntryOverview(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.catalog.status).toBe("ready"));

    await act(async () => { await result.current.register("/work/third"); });
    expect(result.current.catalog.repositories.map((entry) => entry.id)).toEqual([3, 1, 2]);

    await act(async () => { await result.current.register("/work/second"); });
    expect(result.current.catalog.repositories.map((entry) => entry.id)).toEqual([3, 1, 2]);
    expect(result.current.catalog.repositories[2].available).toBe(false);
  });

  it("刷新 Review summaries 时只更新当前 Repository Entry Overview", async () => {
    const adapter = createAdapter();
    const review = {
      reviewId: "review-1", repositoryEntryId: 1, repositoryPath: "/work/first",
      baseRef: "main", headRef: "topic", lastOpenedAt: 7,
    };
    vi.mocked(adapter.loadReviews).mockResolvedValue({ reviews: [review] });
    const { result } = renderHook(() => useRepositoryEntryOverview(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.catalog.status).toBe("ready"));
    await act(async () => { await result.current.open(firstRepository.id); });

    await act(async () => { await result.current.refreshReviews(); });

    expect(result.current.current).toEqual({
      repository: firstRepository,
      reviews: [review],
    });
    expect(adapter.loadReviews).toHaveBeenCalledWith(firstRepository.id);
  });

  it("取消 Trust 或离开 Repository 时不泄漏待处理状态和 Branch 请求", async () => {
    const adapter = createAdapter();
    vi.mocked(adapter.open)
      .mockResolvedValueOnce({ kind: "trust-required", repository: firstRepository })
      .mockResolvedValueOnce({ kind: "ready", overview: { repository: firstRepository, reviews: [] } });
    const { result } = renderHook(() => useRepositoryEntryOverview(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.catalog.status).toBe("ready"));
    await act(async () => { await result.current.open(firstRepository.id); });
    act(() => result.current.dismissTrust());
    expect(result.current.transition).toEqual({ kind: "stable" });

    await act(async () => { await result.current.open(firstRepository.id); });
    await act(async () => { await result.current.reloadBranches(); });
    act(() => result.current.leave());

    expect(result.current.current).toBeNull();
    expect(result.current.branches).toEqual({ kind: "idle" });
  });

  it("重复打开当前 Repository 时复用 Overview 且不更新时间", async () => {
    const adapter = createAdapter();
    const { result } = renderHook(() => useRepositoryEntryOverview(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.catalog.status).toBe("ready"));
    await act(async () => { await result.current.open(firstRepository.id); });
    vi.mocked(adapter.open).mockClear();

    let outcome!: unknown;
    await act(async () => { outcome = await result.current.open(firstRepository.id); });

    expect(adapter.open).not.toHaveBeenCalled();
    expect(outcome).toEqual({ kind: "ready", overview: result.current.current });
  });

  it("交错打开 Repository 时只允许最新目标提交 Overview", async () => {
    const adapter = createAdapter();
    let finishFirst!: () => void;
    vi.mocked(adapter.open).mockImplementation(async (repositoryEntryId) => {
      if (repositoryEntryId === firstRepository.id) {
        await new Promise<void>((resolve) => { finishFirst = resolve; });
      }
      const repository = repositoryEntryId === firstRepository.id ? firstRepository : secondRepository;
      return { kind: "ready", overview: { repository, reviews: [] } };
    });
    const { result } = renderHook(() => useRepositoryEntryOverview(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.catalog.status).toBe("ready"));

    let firstOpen!: Promise<unknown>;
    await act(async () => {
      firstOpen = result.current.open(firstRepository.id);
      await Promise.resolve();
      await result.current.open(secondRepository.id);
    });
    expect(result.current.current?.repository).toBe(secondRepository);

    await act(async () => {
      finishFirst();
      await firstOpen;
    });
    expect(result.current.current?.repository).toBe(secondRepository);
  });
});

import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useAppShellNavigation } from "@/app/use-app-shell-navigation";
import type { RepositoryApplicationAdapter, RepositorySummary } from "@/repository/repository-application";
import { useRepositoryOverview } from "@/repository/use-repository-overview";
import type { ReviewApplicationAdapter } from "@/review/review-application";
import type { ReviewSnapshot } from "@/review/types";
import { useReviewLifecycle } from "@/review/use-review-lifecycle";

const repositories: RepositorySummary[] = [
  { id: 1, canonicalPath: "/work/one", gitCommonDirectory: "/work", createdAtMillis: 1, lastOpenedAtMillis: 1, availability: { kind: "available" } },
  { id: 2, canonicalPath: "/work/two", gitCommonDirectory: "/work", createdAtMillis: 2, lastOpenedAtMillis: 2, availability: { kind: "available" } },
];

function snapshot(reviewId: string, repositoryId: number): ReviewSnapshot {
  return {
    reviewId,
    snapshotId: `${reviewId}-snapshot`,
    baseRef: "refs/heads/main",
    baseRevision: `base-${repositoryId}`,
    comparisonBase: `base-${repositoryId}`,
    headRef: "refs/heads/topic",
    headRevision: `head-${repositoryId}`,
    fileChanges: [],
  };
}

function createRepositoryAdapter(options: { trusted?: Set<number> } = {}): RepositoryApplicationAdapter {
  const trusted = options.trusted ?? new Set(repositories.map((repository) => repository.id));
  return {
    loadNavigation: vi.fn(async () => ({ repositories, recentReviews: [] })),
    register: vi.fn(async () => repositories[0]),
    open: vi.fn(async (repositoryId) => {
      const repository = repositories.find((candidate) => candidate.id === repositoryId)!;
      return trusted.has(repositoryId)
        ? { kind: "ready" as const, overview: { repository, reviews: [] } }
        : { kind: "trust-required" as const, repository };
    }),
    grantTrust: vi.fn(async (repository) => {
      trusted.add(repository.id);
      return { repository, reviews: [] };
    }),
    loadReviews: vi.fn(async () => ({ reviews: [] })),
    loadBranches: vi.fn(async () => ({ refs: [], defaultRef: "", currentRef: "" })),
  };
}

function createReviewAdapter(): ReviewApplicationAdapter & { failedReviewId?: string } {
  const adapter: ReviewApplicationAdapter & { failedReviewId?: string } = {
    async switchReview(target) {
      const reviewId = target.kind === "saved-review" ? target.reviewId : `review-${target.repositoryId}`;
      if (reviewId === adapter.failedReviewId) throw new Error("无法打开目标 Review");
      const repositoryId = target.kind === "saved-review"
        ? Number(reviewId.split("-").at(-1))
        : target.repositoryId;
      return snapshot(reviewId, repositoryId);
    },
    async checkReviewUpdate() { return null; },
    async checkBranchSelectionUpdate() {
      return { changed: false, currentBaseRevision: "base", currentHeadRevision: "head", latestBaseRevision: "base", latestHeadRevision: "head" };
    },
    async refreshReview() { return { kind: "unchanged" }; },
    async readFileChange() { throw new Error("没有 File Change"); },
    async closeReview() { return { kind: "closed" }; },
  };
  return adapter;
}

function renderNavigation(repositoryAdapter: RepositoryApplicationAdapter, reviewAdapter: ReviewApplicationAdapter) {
  return renderHook(() => {
    const repository = useRepositoryOverview(repositoryAdapter, { enabled: true });
    const review = useReviewLifecycle(reviewAdapter);
    return useAppShellNavigation(repository, review);
  });
}

describe("AppShell navigation module", () => {
  it("跨 Repository 打开 Review，并以一个 destination 提交导航结果", async () => {
    const repositoryAdapter = createRepositoryAdapter();
    const reviewAdapter = createReviewAdapter();
    const { result } = renderNavigation(repositoryAdapter, reviewAdapter);
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));

    await act(async () => {
      const outcome = await result.current.navigate({
        kind: "review",
        repositoryId: 2,
        target: { kind: "saved-review", reviewId: "review-2" },
      });
      expect(outcome.kind).toBe("navigated");
    });

    expect(result.current.destination).toEqual({ kind: "review", repositoryId: 2, reviewId: "review-2" });
  });

  it("Workspace Trust 暂停完整导航意图，确认后从同一意图继续", async () => {
    const repositoryAdapter = createRepositoryAdapter({ trusted: new Set([1]) });
    const { result } = renderNavigation(repositoryAdapter, createReviewAdapter());
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));

    await act(async () => {
      const outcome = await result.current.navigate({
        kind: "review",
        repositoryId: 2,
        target: { kind: "saved-review", reviewId: "review-2" },
      });
      expect(outcome.kind).toBe("trust-required");
    });
    expect(result.current.trust?.repository.id).toBe(2);
    expect(result.current.destination).toEqual({ kind: "home" });

    await act(async () => {
      const outcome = await result.current.confirmTrust();
      expect(outcome.kind).toBe("navigated");
    });
    expect(result.current.destination).toEqual({ kind: "review", repositoryId: 2, reviewId: "review-2" });
    expect(result.current.trust).toBeNull();
  });

  it("Repository 导航也通过同一个 Trust 恢复点提交", async () => {
    const repositoryAdapter = createRepositoryAdapter({ trusted: new Set([1]) });
    const { result } = renderNavigation(repositoryAdapter, createReviewAdapter());
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));

    await act(async () => {
      expect((await result.current.navigate({ kind: "repository", repositoryId: 2 })).kind).toBe("trust-required");
    });
    expect(result.current.destination).toEqual({ kind: "home" });

    await act(async () => {
      expect((await result.current.confirmTrust()).kind).toBe("navigated");
    });
    expect(result.current.destination).toEqual({ kind: "repository", repositoryId: 2, initialTab: "pull-requests" });
  });

  it("跨 Repository Review 打开失败时恢复原 Repository 与原 Review", async () => {
    const repositoryAdapter = createRepositoryAdapter();
    const reviewAdapter = createReviewAdapter();
    const { result } = renderNavigation(repositoryAdapter, reviewAdapter);
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));
    await act(async () => {
      await result.current.navigate({
        kind: "review",
        repositoryId: 1,
        target: { kind: "saved-review", reviewId: "review-1" },
      });
    });
    reviewAdapter.failedReviewId = "review-2";

    await act(async () => {
      const outcome = await result.current.navigate({
        kind: "review",
        repositoryId: 2,
        target: { kind: "saved-review", reviewId: "review-2" },
      });
      expect(outcome).toMatchObject({ kind: "failed", previousDestinationPreserved: true });
    });

    expect(result.current.destination).toEqual({ kind: "review", repositoryId: 1, reviewId: "review-1" });
    expect(result.current.reader?.snapshot.reviewId).toBe("review-1");
    expect(repositoryAdapter.open).toHaveBeenLastCalledWith(1);
  });

  it("返回 Home 时只离开当前展示，不关闭 Review runtime", async () => {
    const repositoryAdapter = createRepositoryAdapter();
    const reviewAdapter = createReviewAdapter();
    const closeReview = vi.spyOn(reviewAdapter, "closeReview");
    const { result } = renderNavigation(repositoryAdapter, reviewAdapter);
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));
    await act(async () => {
      await result.current.navigate({
        kind: "review",
        repositoryId: 1,
        target: { kind: "saved-review", reviewId: "review-1" },
      });
      await result.current.navigate({ kind: "home" });
    });

    expect(result.current.destination).toEqual({ kind: "home" });
    expect(result.current.reader).toBeNull();
    expect(closeReview).not.toHaveBeenCalled();
  });

  it("返回 Repository Overview 时只离开当前展示，不关闭 Review runtime", async () => {
    const repositoryAdapter = createRepositoryAdapter();
    const reviewAdapter = createReviewAdapter();
    const closeReview = vi.spyOn(reviewAdapter, "closeReview");
    const { result } = renderNavigation(repositoryAdapter, reviewAdapter);
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));
    await act(async () => {
      await result.current.navigate({
        kind: "review",
        repositoryId: 1,
        target: { kind: "saved-review", reviewId: "review-1" },
      });
    });
    await waitFor(() => expect(result.current.destination.kind).toBe("review"));
    await act(async () => {
      await result.current.navigate({ kind: "repository", repositoryId: 1 });
    });

    expect(result.current.destination).toEqual({ kind: "repository", repositoryId: 1, initialTab: "history" });
    expect(result.current.reader).toBeNull();
    expect(closeReview).not.toHaveBeenCalled();
  });

  it("Settings 是独立目的地，返回时恢复进入前的 Repository", async () => {
    const { result } = renderNavigation(createRepositoryAdapter(), createReviewAdapter());
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));
    await act(async () => {
      await result.current.navigate({ kind: "repository", repositoryId: 1 });
    });
    await act(async () => {
      await result.current.navigate({ kind: "settings" });
    });

    expect(result.current.destination).toEqual({ kind: "settings" });

    await act(async () => {
      await result.current.returnFromSettings();
    });

    expect(result.current.destination).toEqual({ kind: "repository", repositoryId: 1, initialTab: "pull-requests" });
  });

  it("Babysit 恢复入口会保留目标设置分区", async () => {
    const { result } = renderNavigation(createRepositoryAdapter(), createReviewAdapter());
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));

    await act(async () => {
      await result.current.navigate({ kind: "settings", initialSection: "babysit" });
    });

    expect(result.current.destination).toEqual({ kind: "settings", initialSection: "babysit" });
  });

  it("从 Review 返回 Settings 后恢复同一个 Presented Review", async () => {
    const reviewAdapter = createReviewAdapter();
    const switchReview = vi.spyOn(reviewAdapter, "switchReview");
    const { result } = renderNavigation(createRepositoryAdapter(), reviewAdapter);
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));
    await act(async () => {
      await result.current.navigate({ kind: "review", repositoryId: 1, target: { kind: "saved-review", reviewId: "review-1" } });
    });
    await act(async () => {
      await result.current.navigate({ kind: "settings" });
    });
    await act(async () => {
      await result.current.returnFromSettings();
    });

    expect(result.current.destination).toEqual({ kind: "review", repositoryId: 1, reviewId: "review-1" });
    expect(result.current.reader?.snapshot.reviewId).toBe("review-1");
    expect(switchReview).toHaveBeenCalledTimes(1);
  });

  it("重复点击当前 Presented Review 不调用 adapter", async () => {
    const repositoryAdapter = createRepositoryAdapter();
    const reviewAdapter = createReviewAdapter();
    const switchReview = vi.spyOn(reviewAdapter, "switchReview");
    const { result } = renderNavigation(repositoryAdapter, reviewAdapter);
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));
    await act(async () => {
      await result.current.navigate({
        kind: "review",
        repositoryId: 1,
        target: { kind: "saved-review", reviewId: "review-1" },
      });
    });
    switchReview.mockClear();

    await act(async () => {
      expect(await result.current.navigate({
        kind: "review",
        repositoryId: 1,
        target: { kind: "saved-review", reviewId: "review-1" },
      })).toEqual({ kind: "navigated" });
    });

    expect(switchReview).not.toHaveBeenCalled();
  });

  it("真实 A ↔ B presentation 只更新内存 recent 排序，重复 current 不更新", async () => {
    const repositoryAdapter = createRepositoryAdapter();
    const reviewAdapter = createReviewAdapter();
    const switchReview = vi.spyOn(reviewAdapter, "switchReview");
    const loadReviews = vi.mocked(repositoryAdapter.loadReviews);
    const { result } = renderNavigation(repositoryAdapter, reviewAdapter);
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));

    await act(async () => {
      await result.current.navigate({
        kind: "review", repositoryId: 1, target: { kind: "saved-review", reviewId: "review-1" },
      });
      await result.current.navigate({
        kind: "review", repositoryId: 1, target: { kind: "saved-review", reviewId: "review-2" },
      });
    });
    expect(switchReview).toHaveBeenCalledTimes(2);
    expect(result.current.reader?.snapshot.reviewId).toBe("review-2");
    expect(result.current.sidebar.reviews.map((recent) => recent.review.reviewId)).toEqual(["review-2", "review-1"]);
    expect(loadReviews).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.navigate({
        kind: "review", repositoryId: 1, target: { kind: "saved-review", reviewId: "review-1" },
      });
    });
    expect(switchReview).toHaveBeenCalledTimes(3);
    expect(result.current.reader?.snapshot.reviewId).toBe("review-1");
    expect(result.current.sidebar.reviews.map((recent) => recent.review.reviewId)).toEqual(["review-1", "review-2"]);
    expect(loadReviews).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.navigate({
        kind: "review", repositoryId: 1, target: { kind: "saved-review", reviewId: "review-1" },
      });
    });
    expect(switchReview).toHaveBeenCalledTimes(3);
    expect(loadReviews).not.toHaveBeenCalled();
    expect(result.current.sidebar.reviews.map((recent) => recent.review.reviewId)).toEqual(["review-1", "review-2"]);
  });
});

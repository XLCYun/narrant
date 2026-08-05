import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useAppShellNavigation } from "@/app/use-app-shell-navigation";
import type { RepositoryApplicationAdapter, RepositoryEntry } from "@/repository/repository-application";
import { useRepositoryEntryContext } from "@/repository/use-repository-entry-context";
import type { ReviewApplicationAdapter } from "@/review/review-application";
import type { ReviewSnapshot } from "@/review/types";
import { useReviewLifecycle } from "@/review/use-review-lifecycle";

const repositories: RepositoryEntry[] = [
  { id: 1, canonicalPath: "/work/one", commonDirectory: "/work", createdAt: 1, lastOpenedAt: 1, available: true, unavailableReason: null },
  { id: 2, canonicalPath: "/work/two", commonDirectory: "/work", createdAt: 2, lastOpenedAt: 2, available: true, unavailableReason: null },
];

function snapshot(reviewId: string, repositoryEntryId: number): ReviewSnapshot {
  return {
    reviewId,
    snapshotId: `${reviewId}-snapshot`,
    baseRef: "refs/heads/main",
    baseRevision: `base-${repositoryEntryId}`,
    comparisonBase: `base-${repositoryEntryId}`,
    headRef: "refs/heads/topic",
    headRevision: `head-${repositoryEntryId}`,
    fileChanges: [],
  };
}

function createRepositoryAdapter(options: { trusted?: Set<number> } = {}): RepositoryApplicationAdapter {
  const trusted = options.trusted ?? new Set(repositories.map((repository) => repository.id));
  return {
    loadCatalog: vi.fn(async () => ({ repositories, recentReviews: [] })),
    register: vi.fn(async () => repositories[0]),
    open: vi.fn(async (repositoryEntryId) => {
      const repository = repositories.find((candidate) => candidate.id === repositoryEntryId)!;
      return trusted.has(repositoryEntryId)
        ? { kind: "ready" as const, context: { repository, savedReviews: [], reviews: [] } }
        : { kind: "trust-required" as const, repository };
    }),
    grantTrust: vi.fn(async (repository) => {
      trusted.add(repository.id);
      return { repository, savedReviews: [], reviews: [] };
    }),
    loadReviews: vi.fn(async () => ({ savedReviews: [], reviews: [] })),
    loadBranches: vi.fn(async () => ({ refs: [], defaultRef: "", currentRef: "" })),
    markOpened: vi.fn(async (repositoryEntryId) => repositories.find((candidate) => candidate.id === repositoryEntryId)!),
  };
}

function createReviewAdapter(): ReviewApplicationAdapter & { failedReviewId?: string } {
  const adapter: ReviewApplicationAdapter & { failedReviewId?: string } = {
    async switchReview(target) {
      const reviewId = target.kind === "saved-review" ? target.reviewId : `review-${target.repositoryEntryId}`;
      if (reviewId === adapter.failedReviewId) throw new Error("无法打开目标 Review");
      const repositoryEntryId = target.kind === "saved-review"
        ? Number(reviewId.split("-").at(-1))
        : target.repositoryEntryId;
      return snapshot(reviewId, repositoryEntryId);
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
    const repository = useRepositoryEntryContext(repositoryAdapter, { enabled: true });
    const review = useReviewLifecycle(reviewAdapter);
    return useAppShellNavigation(repository, review, { waitForPaint: async () => undefined });
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
        repositoryEntryId: 2,
        target: { kind: "saved-review", reviewId: "review-2" },
      });
      expect(outcome.kind).toBe("navigated");
    });

    expect(result.current.destination).toEqual({ kind: "review", repositoryEntryId: 2, reviewId: "review-2" });
    expect(repositoryAdapter.markOpened).toHaveBeenCalledWith(2);
  });

  it("Workspace Trust 暂停完整导航意图，确认后从同一意图继续", async () => {
    const repositoryAdapter = createRepositoryAdapter({ trusted: new Set([1]) });
    const { result } = renderNavigation(repositoryAdapter, createReviewAdapter());
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));

    await act(async () => {
      const outcome = await result.current.navigate({
        kind: "review",
        repositoryEntryId: 2,
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
    expect(result.current.destination).toEqual({ kind: "review", repositoryEntryId: 2, reviewId: "review-2" });
    expect(result.current.trust).toBeNull();
  });

  it("Repository 导航也通过同一个 Trust 恢复点提交", async () => {
    const repositoryAdapter = createRepositoryAdapter({ trusted: new Set([1]) });
    const { result } = renderNavigation(repositoryAdapter, createReviewAdapter());
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));

    await act(async () => {
      expect((await result.current.navigate({ kind: "repository", repositoryEntryId: 2 })).kind).toBe("trust-required");
    });
    expect(result.current.destination).toEqual({ kind: "home" });

    await act(async () => {
      expect((await result.current.confirmTrust()).kind).toBe("navigated");
    });
    expect(result.current.destination).toEqual({ kind: "repository", repositoryEntryId: 2, initialTab: "pull-requests" });
  });

  it("跨 Repository Review 打开失败时恢复原 Repository 与原 Review", async () => {
    const repositoryAdapter = createRepositoryAdapter();
    const reviewAdapter = createReviewAdapter();
    const { result } = renderNavigation(repositoryAdapter, reviewAdapter);
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));
    await act(async () => {
      await result.current.navigate({
        kind: "review",
        repositoryEntryId: 1,
        target: { kind: "saved-review", reviewId: "review-1" },
      });
    });
    reviewAdapter.failedReviewId = "review-2";

    await act(async () => {
      const outcome = await result.current.navigate({
        kind: "review",
        repositoryEntryId: 2,
        target: { kind: "saved-review", reviewId: "review-2" },
      });
      expect(outcome).toMatchObject({ kind: "failed", previousDestinationPreserved: true });
    });

    expect(result.current.destination).toEqual({ kind: "review", repositoryEntryId: 1, reviewId: "review-1" });
    expect(result.current.reader?.snapshot.reviewId).toBe("review-1");
    expect(repositoryAdapter.open).toHaveBeenLastCalledWith(1);
  });

  it("返回 Home 时统一关闭 Review 并离开 Repository Entry", async () => {
    const repositoryAdapter = createRepositoryAdapter();
    const reviewAdapter = createReviewAdapter();
    const closeReview = vi.spyOn(reviewAdapter, "closeReview");
    const { result } = renderNavigation(repositoryAdapter, reviewAdapter);
    await waitFor(() => expect(result.current.sidebar.loading).toBe(false));
    await act(async () => {
      await result.current.navigate({
        kind: "review",
        repositoryEntryId: 1,
        target: { kind: "saved-review", reviewId: "review-1" },
      });
      await result.current.navigate({ kind: "home" });
    });

    expect(result.current.destination).toEqual({ kind: "home" });
    expect(result.current.reader).toBeNull();
    expect(closeReview).toHaveBeenCalledTimes(1);
  });
});

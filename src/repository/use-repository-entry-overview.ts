import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  OpenRepositoryResult,
  RepositoryApplicationAdapter,
  BranchRefs,
  RepositoryEntryOverview,
  RepositoryEntry,
} from "@/repository/repository-application";
import type { RecentReviewSummary } from "@/lib/review-types";

export type RepositoryCatalogState = {
  repositories: RepositoryEntry[];
  recentReviews: RecentReviewSummary[];
  status: "idle" | "loading" | "ready" | "failed";
  error?: string;
};

export type RepositoryEntryTransition =
  | { kind: "stable" }
  | { kind: "registering"; path: string }
  | { kind: "opening"; repositoryEntryId: number }
  | { kind: "trust-required"; repository: RepositoryEntry }
  | { kind: "granting-trust"; repository: RepositoryEntry };

export type RepositoryEntryOverviewOptions = {
  enabled: boolean;
};

export type RepositoryBranchesState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; value: BranchRefs }
  | { kind: "failed"; error: string };

export type RepositoryEntryOverviewBinding = {
  catalog: RepositoryCatalogState;
  current: RepositoryEntryOverview | null;
  transition: RepositoryEntryTransition;
  branches: RepositoryBranchesState;
  register(path: string): Promise<RepositoryEntry>;
  open(repositoryEntryId: number): Promise<RepositoryEntryOpenResult>;
  confirmTrust(): Promise<RepositoryEntryOverview | undefined>;
  dismissTrust(): void;
  reloadBranches(repositoryEntryId?: number): Promise<void>;
  refreshReviews(): Promise<void>;
  recordReviewOpened(review: RecentReviewSummary): void;
  leave(): void;
};

export type RepositoryEntryOpenResult = OpenRepositoryResult | { kind: "superseded" };

/** 管理 Repository Catalog 与当前 Repository Entry Overview 的原子切换。 */
export function useRepositoryEntryOverview(
  adapter: RepositoryApplicationAdapter,
  options: RepositoryEntryOverviewOptions,
): RepositoryEntryOverviewBinding {
  const [catalog, setCatalog] = useState<RepositoryCatalogState>({
    repositories: [],
    recentReviews: [],
    status: options.enabled ? "loading" : "idle",
  });
  const [current, setCurrent] = useState<RepositoryEntryOverview | null>(null);
  const [transition, setTransition] = useState<RepositoryEntryTransition>({ kind: "stable" });
  const [branches, setBranches] = useState<RepositoryBranchesState>({ kind: "idle" });
  const mounted = useRef(true);
  const currentRef = useRef<RepositoryEntryOverview | null>(null);
  const branchAbort = useRef<AbortController>();
  const branchRequest = useRef(0);
  const openRequest = useRef(0);

  /** 同步提交 React state 与异步操作读取的当前 Repository Entry Overview。 */
  const presentOverview = useCallback((overview: RepositoryEntryOverview) => {
    currentRef.current = overview;
    setCurrent(overview);
    setBranches({ kind: "idle" });
    setCatalog((value) => ({
      ...value,
      repositories: value.repositories.map((entry) => entry.id === overview.repository.id
        ? overview.repository
        : entry),
    }));
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      branchAbort.current?.abort();
    };
  }, []);

  useEffect(() => {
    if (!options.enabled) return;
    setCatalog((value) => ({ ...value, status: "loading", error: undefined }));
    void adapter.loadCatalog()
      .then(({ repositories, recentReviews }) => {
        if (!mounted.current) return;
        setCatalog({ repositories: repositories ?? [], recentReviews: recentReviews ?? [], status: "ready" });
      })
      .catch((reason) => {
        if (!mounted.current) return;
        setCatalog((value) => ({ ...value, status: "failed", error: String(reason) }));
      });
  }, [adapter, options.enabled]);

  const open = useCallback(async (repositoryEntryId: number): Promise<RepositoryEntryOpenResult> => {
    if (currentRef.current?.repository.id === repositoryEntryId) {
      openRequest.current += 1;
      setTransition({ kind: "stable" });
      return { kind: "ready", overview: currentRef.current };
    }
    branchAbort.current?.abort();
    branchRequest.current += 1;
    const request = ++openRequest.current;
    setTransition({ kind: "opening", repositoryEntryId });
    try {
      const result = await adapter.open(repositoryEntryId);
      if (openRequest.current !== request) return { kind: "superseded" };
      if (result.kind === "ready") {
        presentOverview(result.overview);
        setTransition({ kind: "stable" });
      } else if (result.kind === "trust-required") {
        setTransition({ kind: "trust-required", repository: result.repository });
      } else {
        setTransition({ kind: "stable" });
      }
      return result;
    } catch (reason) {
      if (mounted.current && openRequest.current === request) setTransition({ kind: "stable" });
      throw reason;
    }
  }, [adapter, presentOverview]);

  /** 注册具体本地 clone；新 Entry 前置，已有 Entry 仅原位更新。 */
  const register = useCallback(async (path: string): Promise<RepositoryEntry> => {
    setTransition({ kind: "registering", path });
    try {
      const repository = await adapter.register(path);
      setCatalog((value) => ({
        ...value,
        repositories: value.repositories.some((entry) => entry.id === repository.id)
          ? value.repositories.map((entry) => entry.id === repository.id ? repository : entry)
          : [repository, ...value.repositories],
      }));
      setTransition({ kind: "stable" });
      return repository;
    } catch (reason) {
      if (mounted.current) setTransition({ kind: "stable" });
      throw reason;
    }
  }, [adapter]);

  /** 确认当前待授权 Repository，并只在完整 Overview 返回后提交切换。 */
  const confirmTrust = useCallback(async (): Promise<RepositoryEntryOverview | undefined> => {
    if (transition.kind !== "trust-required") return undefined;
    const repository = transition.repository;
    const request = ++openRequest.current;
    setTransition({ kind: "granting-trust", repository });
    try {
      const overview = await adapter.grantTrust(repository);
      if (openRequest.current !== request) return undefined;
      presentOverview(overview);
      setTransition({ kind: "stable" });
      return overview;
    } catch (reason) {
      if (mounted.current && openRequest.current === request) {
        setTransition({ kind: "trust-required", repository });
      }
      throw reason;
    }
  }, [adapter, presentOverview, transition]);

  /** 加载当前或显式 Repository 的 canonical Branch refs，并隔离已取消请求的迟到结果。 */
  const reloadBranches = useCallback(async (repositoryEntryId?: number): Promise<void> => {
    const overview = currentRef.current;
    const resolvedRepositoryEntryId = repositoryEntryId ?? overview?.repository.id;
    if (resolvedRepositoryEntryId === undefined) return;
    branchAbort.current?.abort();
    const abort = new AbortController();
    branchAbort.current = abort;
    const request = ++branchRequest.current;
    setBranches({ kind: "loading" });
    try {
      const value = await adapter.loadBranches(resolvedRepositoryEntryId, abort.signal);
      if (branchRequest.current === request && !abort.signal.aborted) setBranches({ kind: "ready", value });
    } catch (reason) {
      if (branchRequest.current === request && !abort.signal.aborted) {
        setBranches({ kind: "failed", error: String(reason) });
      }
    } finally {
      if (branchAbort.current === abort) branchAbort.current = undefined;
    }
  }, [adapter]);

  /** 一次性同步 Review 最近打开记录及其所属 Repository Entry 摘要。 */
  const recordReviewOpened = useCallback((recent: RecentReviewSummary) => {
    const review = recent.review;
    const overview = currentRef.current;
    if (overview?.repository.id === recent.repositoryEntryId) {
      const nextOverview = {
        ...overview,
        repository: { ...overview.repository, lastOpenedAt: review.lastOpenedAt },
        reviews: [review, ...overview.reviews.filter((candidate) => candidate.reviewId !== review.reviewId)],
      };
      currentRef.current = nextOverview;
      setCurrent(nextOverview);
    }
    setCatalog((value) => ({
      ...value,
      recentReviews: [recent, ...value.recentReviews.filter((candidate) => candidate.review.reviewId !== review.reviewId)],
      repositories: value.repositories.map((entry) => entry.id === recent.repositoryEntryId
        ? { ...entry, lastOpenedAt: review.lastOpenedAt }
        : entry),
    }));
  }, []);

  /** 重新读取当前 Repository 的持久化 Review summaries。 */
  const refreshReviews = useCallback(async (): Promise<void> => {
    const overview = currentRef.current;
    if (!overview) return;
    const repositoryEntryId = overview.repository.id;
    const loaded = await adapter.loadReviews(repositoryEntryId);
    if (currentRef.current !== overview) return;
    const nextOverview = { ...currentRef.current, ...loaded };
    currentRef.current = nextOverview;
    setCurrent(nextOverview);
  }, [adapter]);

  /** 放弃当前 Workspace Trust 请求但保留已呈现的 Repository Entry Overview。 */
  const dismissTrust = useCallback(() => {
    setTransition((value) => value.kind === "trust-required" ? { kind: "stable" } : value);
  }, []);

  /** 离开 Repository Entry，并取消所有 Repository-scoped 后台读取。 */
  const leave = useCallback(() => {
    branchAbort.current?.abort();
    branchRequest.current += 1;
    openRequest.current += 1;
    currentRef.current = null;
    setCurrent(null);
    setBranches({ kind: "idle" });
    setTransition({ kind: "stable" });
  }, []);

  return useMemo(
    () => ({
      catalog,
      current,
      transition,
      branches,
      register,
      open,
      confirmTrust,
      reloadBranches,
      recordReviewOpened,
      refreshReviews,
      dismissTrust,
      leave,
    }),
    [branches, catalog, confirmTrust, current, dismissTrust, leave, open, recordReviewOpened, refreshReviews, register, reloadBranches, transition],
  );
}

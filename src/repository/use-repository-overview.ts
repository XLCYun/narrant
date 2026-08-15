import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  OpenRepositoryResult,
  RepositoryApplicationAdapter,
  BranchRefs,
  RepositoryOverview,
  RepositorySummary,
} from "@/repository/repository-application";
import type { RecentReviewSummary } from "@/lib/review-types";

export type RepositoryNavigationState = {
  repositories: RepositorySummary[];
  recentReviews: RecentReviewSummary[];
  status: "idle" | "loading" | "ready" | "failed";
  error?: string;
};

export type RepositoryTransition =
  | { kind: "stable" }
  | { kind: "registering"; path: string }
  | { kind: "opening"; repositoryId: number }
  | { kind: "trust-required"; repository: RepositorySummary }
  | { kind: "granting-trust"; repository: RepositorySummary };

export type RepositoryOverviewOptions = {
  enabled: boolean;
};

export type RepositoryBranchesState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; value: BranchRefs }
  | { kind: "failed"; error: string };

export type RepositoryOverviewBinding = {
  navigation: RepositoryNavigationState;
  current: RepositoryOverview | null;
  transition: RepositoryTransition;
  branches: RepositoryBranchesState;
  register(path: string): Promise<RepositorySummary>;
  open(repositoryId: number): Promise<RepositoryOpenResult>;
  confirmTrust(): Promise<RepositoryOverview | undefined>;
  dismissTrust(): void;
  reloadBranches(repositoryId?: number): Promise<void>;
  refreshReviews(): Promise<void>;
  recordReviewOpened(review: RecentReviewSummary): void;
  leave(): void;
};

export type RepositoryOpenResult = OpenRepositoryResult | { kind: "superseded" };

/** 管理 Repository Navigation 与当前 Repository Overview 的原子切换。 */
export function useRepositoryOverview(
  adapter: RepositoryApplicationAdapter,
  options: RepositoryOverviewOptions,
): RepositoryOverviewBinding {
  const [navigation, setNavigation] = useState<RepositoryNavigationState>({
    repositories: [],
    recentReviews: [],
    status: options.enabled ? "loading" : "idle",
  });
  const [current, setCurrent] = useState<RepositoryOverview | null>(null);
  const [transition, setTransition] = useState<RepositoryTransition>({ kind: "stable" });
  const [branches, setBranches] = useState<RepositoryBranchesState>({ kind: "idle" });
  const mounted = useRef(true);
  const currentRef = useRef<RepositoryOverview | null>(null);
  const branchAbort = useRef<AbortController>();
  const branchRequest = useRef(0);
  const openRequest = useRef(0);

  /** 同步提交 React state 与异步操作读取的当前 Repository Overview。 */
  const presentOverview = useCallback((overview: RepositoryOverview) => {
    currentRef.current = overview;
    setCurrent(overview);
    setBranches({ kind: "idle" });
    setNavigation((value) => ({
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
    setNavigation((value) => ({ ...value, status: "loading", error: undefined }));
    void adapter.loadNavigation()
      .then(({ repositories, recentReviews }) => {
        if (!mounted.current) return;
        setNavigation({ repositories: repositories ?? [], recentReviews: recentReviews ?? [], status: "ready" });
      })
      .catch((reason) => {
        if (!mounted.current) return;
        setNavigation((value) => ({ ...value, status: "failed", error: String(reason) }));
      });
  }, [adapter, options.enabled]);

  const open = useCallback(async (repositoryId: number): Promise<RepositoryOpenResult> => {
    if (currentRef.current?.repository.id === repositoryId) {
      openRequest.current += 1;
      setTransition({ kind: "stable" });
      return { kind: "ready", overview: currentRef.current };
    }
    branchAbort.current?.abort();
    branchRequest.current += 1;
    const request = ++openRequest.current;
    setTransition({ kind: "opening", repositoryId });
    try {
      const result = await adapter.open(repositoryId);
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
  const register = useCallback(async (path: string): Promise<RepositorySummary> => {
    setTransition({ kind: "registering", path });
    try {
      const repository = await adapter.register(path);
      setNavigation((value) => ({
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
  const confirmTrust = useCallback(async (): Promise<RepositoryOverview | undefined> => {
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
  const reloadBranches = useCallback(async (repositoryId?: number): Promise<void> => {
    const overview = currentRef.current;
    const resolvedRepositoryId = repositoryId ?? overview?.repository.id;
    if (resolvedRepositoryId === undefined) return;
    branchAbort.current?.abort();
    const abort = new AbortController();
    branchAbort.current = abort;
    const request = ++branchRequest.current;
    setBranches({ kind: "loading" });
    try {
      const value = await adapter.loadBranches(resolvedRepositoryId, abort.signal);
      if (branchRequest.current === request && !abort.signal.aborted) setBranches({ kind: "ready", value });
    } catch (reason) {
      if (branchRequest.current === request && !abort.signal.aborted) {
        setBranches({ kind: "failed", error: String(reason) });
      }
    } finally {
      if (branchAbort.current === abort) branchAbort.current = undefined;
    }
  }, [adapter]);

  /** 一次性同步 Review 最近打开记录及其所属 Repository 摘要。 */
  const recordReviewOpened = useCallback((recent: RecentReviewSummary) => {
    const review = recent.review;
    const overview = currentRef.current;
    if (overview?.repository.id === recent.repositoryId) {
      const nextOverview = {
        ...overview,
        repository: { ...overview.repository, lastOpenedAt: review.lastOpenedAt },
        reviews: [review, ...overview.reviews.filter((candidate) => candidate.reviewId !== review.reviewId)],
      };
      currentRef.current = nextOverview;
      setCurrent(nextOverview);
    }
    setNavigation((value) => ({
      ...value,
      recentReviews: [recent, ...value.recentReviews.filter((candidate) => candidate.review.reviewId !== review.reviewId)],
      repositories: value.repositories.map((entry) => entry.id === recent.repositoryId
        ? { ...entry, lastOpenedAt: review.lastOpenedAt }
        : entry),
    }));
  }, []);

  /** 重新读取当前 Repository 的持久化 Review summaries。 */
  const refreshReviews = useCallback(async (): Promise<void> => {
    const overview = currentRef.current;
    if (!overview) return;
    const repositoryId = overview.repository.id;
    const loaded = await adapter.loadReviews(repositoryId);
    if (currentRef.current !== overview) return;
    const nextOverview = { ...currentRef.current, ...loaded };
    currentRef.current = nextOverview;
    setCurrent(nextOverview);
  }, [adapter]);

  /** 放弃当前 Workspace Trust 请求但保留已呈现的 Repository Overview。 */
  const dismissTrust = useCallback(() => {
    setTransition((value) => value.kind === "trust-required" ? { kind: "stable" } : value);
  }, []);

  /** 离开 Repository，并取消所有 Repository-scoped 后台读取。 */
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
      navigation,
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
    [branches, navigation, confirmTrust, current, dismissTrust, leave, open, recordReviewOpened, refreshReviews, register, reloadBranches, transition],
  );
}

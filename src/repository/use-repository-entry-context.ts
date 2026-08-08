import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  OpenRepositoryResult,
  RepositoryApplicationAdapter,
  BranchRefs,
  RepositoryContext,
  RepositoryEntry,
} from "@/repository/repository-application";
import type { ReviewSummary } from "@/lib/review-types";

export type RepositoryCatalogState = {
  repositories: RepositoryEntry[];
  recentReviews: ReviewSummary[];
  status: "idle" | "loading" | "ready" | "failed";
  error?: string;
};

export type RepositoryEntryTransition =
  | { kind: "stable" }
  | { kind: "registering"; path: string }
  | { kind: "opening"; repositoryEntryId: number }
  | { kind: "trust-required"; repository: RepositoryEntry }
  | { kind: "granting-trust"; repository: RepositoryEntry };

export type RepositoryEntryContextOptions = {
  enabled: boolean;
};

export type RepositoryBranchesState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; value: BranchRefs }
  | { kind: "failed"; error: string };

export type RepositoryEntryContextBinding = {
  catalog: RepositoryCatalogState;
  current: RepositoryContext | null;
  transition: RepositoryEntryTransition;
  branches: RepositoryBranchesState;
  register(path: string): Promise<RepositoryEntry>;
  open(repositoryEntryId: number): Promise<RepositoryEntryOpenResult>;
  confirmTrust(): Promise<RepositoryContext | undefined>;
  dismissTrust(): void;
  reloadBranches(repositoryEntryId?: number): Promise<void>;
  refreshReviews(): Promise<void>;
  recordReviewOpened(review: ReviewSummary): void;
  leave(): void;
};

export type RepositoryEntryOpenResult = OpenRepositoryResult | { kind: "superseded" };

/** 管理 Repository Catalog 与当前 Repository Entry 的原子上下文切换。 */
export function useRepositoryEntryContext(
  adapter: RepositoryApplicationAdapter,
  options: RepositoryEntryContextOptions,
): RepositoryEntryContextBinding {
  const [catalog, setCatalog] = useState<RepositoryCatalogState>({
    repositories: [],
    recentReviews: [],
    status: options.enabled ? "loading" : "idle",
  });
  const [current, setCurrent] = useState<RepositoryContext | null>(null);
  const [transition, setTransition] = useState<RepositoryEntryTransition>({ kind: "stable" });
  const [branches, setBranches] = useState<RepositoryBranchesState>({ kind: "idle" });
  const mounted = useRef(true);
  const currentRef = useRef<RepositoryContext | null>(null);
  const branchAbort = useRef<AbortController>();
  const branchRequest = useRef(0);
  const openRequest = useRef(0);

  /** 同步提交 React state 与异步操作读取的当前 Repository context。 */
  const presentContext = useCallback((context: RepositoryContext) => {
    currentRef.current = context;
    setCurrent(context);
    setBranches({ kind: "idle" });
    setCatalog((value) => ({
      ...value,
      repositories: value.repositories.map((entry) => entry.id === context.repository.id
        ? context.repository
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
      return { kind: "ready", context: currentRef.current };
    }
    branchAbort.current?.abort();
    branchRequest.current += 1;
    const request = ++openRequest.current;
    setTransition({ kind: "opening", repositoryEntryId });
    try {
      const result = await adapter.open(repositoryEntryId);
      if (openRequest.current !== request) return { kind: "superseded" };
      if (result.kind === "ready") {
        presentContext(result.context);
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
  }, [adapter, presentContext]);

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

  /** 确认当前待授权 Repository，并只在完整 context 返回后提交切换。 */
  const confirmTrust = useCallback(async (): Promise<RepositoryContext | undefined> => {
    if (transition.kind !== "trust-required") return undefined;
    const repository = transition.repository;
    const request = ++openRequest.current;
    setTransition({ kind: "granting-trust", repository });
    try {
      const context = await adapter.grantTrust(repository);
      if (openRequest.current !== request) return undefined;
      presentContext(context);
      setTransition({ kind: "stable" });
      return context;
    } catch (reason) {
      if (mounted.current && openRequest.current === request) {
        setTransition({ kind: "trust-required", repository });
      }
      throw reason;
    }
  }, [adapter, presentContext, transition]);

  /** 加载当前或显式 Repository 的 canonical Branch refs，并隔离已取消请求的迟到结果。 */
  const reloadBranches = useCallback(async (repositoryEntryId?: number): Promise<void> => {
    const context = currentRef.current;
    const resolvedRepositoryEntryId = repositoryEntryId ?? context?.repository.id;
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
  const recordReviewOpened = useCallback((review: ReviewSummary) => {
    const context = currentRef.current;
    if (context?.repository.id === review.repositoryEntryId) {
      const nextContext = {
        ...context,
        repository: { ...context.repository, lastOpenedAt: review.lastOpenedAt },
        reviews: [review, ...context.reviews.filter((candidate) => candidate.reviewId !== review.reviewId)],
      };
      currentRef.current = nextContext;
      setCurrent(nextContext);
    }
    setCatalog((value) => ({
      ...value,
      recentReviews: [review, ...value.recentReviews.filter((candidate) => candidate.reviewId !== review.reviewId)],
      repositories: value.repositories.map((entry) => entry.id === review.repositoryEntryId
        ? { ...entry, lastOpenedAt: review.lastOpenedAt }
        : entry),
    }));
  }, []);

  /** 重新读取当前 Repository 的持久化 Review summaries。 */
  const refreshReviews = useCallback(async (): Promise<void> => {
    const context = currentRef.current;
    if (!context) return;
    const repositoryEntryId = context.repository.id;
    const loaded = await adapter.loadReviews(repositoryEntryId);
    if (currentRef.current !== context) return;
    const nextContext = { ...currentRef.current, ...loaded };
    currentRef.current = nextContext;
    setCurrent(nextContext);
  }, [adapter]);

  /** 放弃当前 Workspace Trust 请求但保留已呈现的 Repository context。 */
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

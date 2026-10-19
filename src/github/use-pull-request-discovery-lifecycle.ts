import { useCallback, useEffect, useRef, useState } from "react";

import type { ApplicationError } from "@/app/application-error";
import { applicationError } from "@/app/application-error";
import type { PullRequestDiscoveryApplicationAdapter } from "@/github/pull-request-discovery-application";
import type { DiscoveredPullRequest, PullRequestDiscoveryQuery, PullRequestGroup, PullRequestStateFilter } from "@/lib/review-types";

/** Pull Request Discovery 当前正在执行的转换。 */
export type PullRequestDiscoveryTransition =
  | { kind: "idle" | "loading" | "refreshing" | "loading-more" | "stable" };

/** Pull Request Discovery lifecycle 的输入 identity。 */
export type PullRequestDiscoveryLifecycleOptions = {
  enabled: boolean;
  repositoryId?: number;
  signedIn: boolean;
};

/** Pull Request Discovery lifecycle 暴露给调用方的状态与显式操作。 */
export type PullRequestDiscoveryLifecycleBinding = {
  groups: PullRequestGroup[];
  filter: PullRequestStateFilter;
  transition: PullRequestDiscoveryTransition;
  error?: ApplicationError;
  complete: boolean;
  selectFilter(filter: PullRequestStateFilter): void;
  refresh(): Promise<void>;
  loadMore(): Promise<void>;
  retry(): Promise<void>;
};

/** 一个筛选已经加载的内容与失败请求；不跨 Repository 概览持久化。 */
type QueryState = {
  groups: PullRequestGroup[];
  settled: boolean;
  error?: ApplicationError;
  failedQuery?: PullRequestDiscoveryQuery;
};

/** 为尚未读取的筛选创建明确的未知状态，避免把它当作完整空列表。 */
function emptyQuery(): QueryState {
  return { groups: [], settled: false };
}

/** GitHub repository identity 不区分大小写。 */
function identity(repository: PullRequestGroup["repository"]): string {
  return `${repository.owner}/${repository.name}`.toLowerCase();
}

/** 页码分页可能重复返回移动中的条目；同一 PR 只保留更新时间较新的 metadata。 */
function mergeItems(previous: DiscoveredPullRequest[], incoming: DiscoveredPullRequest[]): DiscoveredPullRequest[] {
  const items = new Map<number, DiscoveredPullRequest>();
  for (const item of [...previous, ...incoming]) {
    const existing = items.get(item.number);
    if (!existing || Date.parse(item.githubUpdatedAt) >= Date.parse(existing.githubUpdatedAt)) {
      items.set(item.number, item);
    }
  }
  return [...items.values()];
}

/** 首页替换、后续页追加；失败保留条目和重试页，移除不再存在的请求来源。 */
function mergeGroups(previous: PullRequestGroup[], incoming: PullRequestGroup[], query: PullRequestDiscoveryQuery): PullRequestGroup[] {
  const old = new Map(previous.map((group) => [identity(group.repository), group]));
  const next = query.pages ? new Map(old) : new Map<string, PullRequestGroup>();
  for (const request of query.pages ?? []) next.delete(identity(request.repository));
  for (const group of incoming) {
    const key = identity(group.repository);
    const cached = old.get(key)?.pullRequests ?? [];
    next.set(key, {
      ...group,
      pullRequests: group.error ? cached : mergeItems(group.page === 1 ? [] : cached, group.pullRequests),
    });
  }
  return [...next.values()];
}

/** 管理四种筛选的独立分页，只有当前 intent 可以发布结果。 */
export function usePullRequestDiscoveryLifecycle(
  adapter: PullRequestDiscoveryApplicationAdapter,
  options: PullRequestDiscoveryLifecycleOptions,
): PullRequestDiscoveryLifecycleBinding {
  const [filter, setFilter] = useState<PullRequestStateFilter>("open");
  const [current, setCurrent] = useState<QueryState>(emptyQuery);
  const [transition, setTransition] = useState<PullRequestDiscoveryTransition>({ kind: "idle" });
  const queriesRef = useRef(new Map<PullRequestStateFilter, QueryState>());
  const filterRef = useRef<PullRequestStateFilter>("open");
  const repositoryRef = useRef<number>();
  const requestRef = useRef(0);
  const inFlightRequestRef = useRef<number>();

  /** 对指定筛选执行有界查询；加载更多不会追读失败来源或已结束的来源。 */
  const load = useCallback(async (state: PullRequestStateFilter, action: "refresh" | "more" | "retry") => {
    if (!options.enabled || options.repositoryId === undefined || inFlightRequestRef.current !== undefined) return;
    const previous = queriesRef.current.get(state) ?? emptyQuery();
    let query: PullRequestDiscoveryQuery = { state };
    if (action === "retry" && previous.failedQuery) {
      query = previous.failedQuery;
    } else if (action === "more" || (action === "retry" && previous.groups.some((group) => group.error))) {
      if (action === "more" && previous.error) return;
      query.pages = previous.groups
        .filter((group) => group.nextPage !== null && (action === "retry" ? Boolean(group.error) : !group.error))
        .map((group) => ({ repository: group.repository, page: group.nextPage! }));
      if (query.pages.length === 0) return;
    }
    const request = ++requestRef.current;
    inFlightRequestRef.current = request;
    setCurrent({ ...previous, error: undefined });
    setTransition({ kind: query.pages ? "loading-more" : (previous.settled || previous.groups.length > 0) ? "refreshing" : "loading" });
    try {
      const discovered = await adapter.discover(options.repositoryId, query);
      if (request !== requestRef.current) return;
      const next: QueryState = { groups: mergeGroups(previous.groups, discovered, query), settled: true };
      queriesRef.current.set(state, next);
      setCurrent(next);
    } catch (reason) {
      if (request !== requestRef.current) return;
      const next: QueryState = {
        ...previous, settled: true, failedQuery: query,
        error: applicationError("pull-request-discovery", "discover", reason),
      };
      queriesRef.current.set(state, next);
      setCurrent(next);
    } finally {
      if (request === requestRef.current) {
        inFlightRequestRef.current = undefined;
        setTransition({ kind: "stable" });
      }
    }
  }, [adapter, options.enabled, options.repositoryId]);

  useEffect(() => {
    const repositoryChanged = repositoryRef.current !== options.repositoryId;
    repositoryRef.current = options.repositoryId;
    requestRef.current += 1;
    inFlightRequestRef.current = undefined;
    if (repositoryChanged || !options.enabled) {
      queriesRef.current.clear();
      filterRef.current = "open";
      setFilter("open");
      setCurrent(emptyQuery());
    }
    if (!options.enabled || options.repositoryId === undefined) {
      setTransition({ kind: "idle" });
      return;
    }
    // 身份变化后保留可用内容，但其他筛选下次进入时也必须重取首页。
    for (const [state, cached] of queriesRef.current) queriesRef.current.set(state, { ...cached, settled: false });
    void load(filterRef.current, "refresh");
    return () => {
      requestRef.current += 1;
      inFlightRequestRef.current = undefined;
    };
  }, [load, options.enabled, options.repositoryId, options.signedIn]);

  /** 切换筛选立即隔离旧请求；已完成的查询复用本次概览内的独立缓存。 */
  const selectFilter = useCallback((state: PullRequestStateFilter) => {
    if (state === filterRef.current) return;
    requestRef.current += 1;
    inFlightRequestRef.current = undefined;
    filterRef.current = state;
    setFilter(state);
    const cached = queriesRef.current.get(state) ?? emptyQuery();
    setCurrent(cached);
    setTransition({ kind: cached.settled ? "stable" : "loading" });
    if (!cached.settled) void load(state, "refresh");
  }, [load]);

  /** 刷新只重取当前筛选首页，成功后丢弃该来源先前加载的后续页。 */
  const refresh = useCallback(() => load(filterRef.current, "refresh"), [load]);
  /** 追加各正常来源的下一页，失败来源由独立重试入口恢复。 */
  const loadMore = useCallback(() => load(filterRef.current, "more"), [load]);
  /** 重试原操作或失败来源的原页，不让成功来源重复翻页。 */
  const retry = useCallback(() => load(filterRef.current, "retry"), [load]);
  const complete = current.settled && transition.kind === "stable" && !current.error
    && current.groups.every((group) => !group.error && group.nextPage === null);
  return { groups: current.groups, filter, transition, error: current.error, complete, selectFilter, refresh, loadMore, retry };
}

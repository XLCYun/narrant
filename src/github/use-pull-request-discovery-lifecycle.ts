import { useCallback, useEffect, useRef, useState } from "react";

import type { ApplicationError } from "@/app/application-error";
import { applicationError } from "@/app/application-error";
import type { PullRequestDiscoveryApplicationAdapter } from "@/github/pull-request-discovery-application";
import type { PullRequestGroup } from "@/lib/review-types";

/** Pull Request Discovery 当前正在执行的转换。 */
export type PullRequestDiscoveryTransition =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "refreshing" }
  | { kind: "stable" };

/** Pull Request Discovery lifecycle 的输入 identity。 */
export type PullRequestDiscoveryLifecycleOptions = {
  enabled: boolean;
  repositoryId?: number;
  signedIn: boolean;
};

/** Pull Request Discovery lifecycle 暴露给调用方的状态。 */
export type PullRequestDiscoveryLifecycleBinding = {
  groups: PullRequestGroup[];
  transition: PullRequestDiscoveryTransition;
  error?: ApplicationError;
  retry(): Promise<void>;
};

/** 仅为本轮仍存在但请求失败的 GitHub repository 保留上次可用条目。 */
function retainFailedGroups(previous: PullRequestGroup[], discovered: PullRequestGroup[]): PullRequestGroup[] {
  return discovered.map((group) => {
    if (!group.error) return group;
    const cached = previous.find((candidate) =>
      candidate.repository.owner.toLowerCase() === group.repository.owner.toLowerCase()
      && candidate.repository.name.toLowerCase() === group.repository.name.toLowerCase());
    return cached ? { ...group, pullRequests: cached.pullRequests } : group;
  });
}

/** 管理 Repository 与 GitHub Identity 变化下的 Pull Request Discovery。 */
export function usePullRequestDiscoveryLifecycle(
  adapter: PullRequestDiscoveryApplicationAdapter,
  options: PullRequestDiscoveryLifecycleOptions,
): PullRequestDiscoveryLifecycleBinding {
  const [groups, setGroups] = useState<PullRequestGroup[]>([]);
  const [transition, setTransition] = useState<PullRequestDiscoveryTransition>({ kind: "idle" });
  const [error, setError] = useState<ApplicationError>();
  const groupsRef = useRef<PullRequestGroup[]>([]);
  const repositoryRef = useRef<number>();
  const requestRef = useRef(0);
  const inFlightRequestRef = useRef<number>();

  /** 加载当前 Repository，并只允许最新 intent 发布结果。 */
  const load = useCallback(async (clear: boolean) => {
    if (!options.enabled || options.repositoryId === undefined) return;
    const request = ++requestRef.current;
    inFlightRequestRef.current = request;
    if (clear) {
      groupsRef.current = [];
      setGroups([]);
    }
    setError(undefined);
    setTransition({ kind: clear || groupsRef.current.length === 0 ? "loading" : "refreshing" });
    try {
      const discovered = await adapter.discover(options.repositoryId);
      if (request !== requestRef.current) return;
      const next = retainFailedGroups(groupsRef.current, discovered);
      groupsRef.current = next;
      setGroups(next);
    } catch (reason) {
      if (request !== requestRef.current) return;
      setError(applicationError("pull-request-discovery", "discover", reason));
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
    if (!options.enabled || options.repositoryId === undefined) {
      requestRef.current += 1;
      inFlightRequestRef.current = undefined;
      groupsRef.current = [];
      setGroups([]);
      setError(undefined);
      setTransition({ kind: "idle" });
      return;
    }
    void load(repositoryChanged);
    return () => {
      requestRef.current += 1;
      inFlightRequestRef.current = undefined;
    };
  }, [load, options.enabled, options.repositoryId, options.signedIn]);

  /** 手动刷新与失败重试共用入口，已有请求进行中时不重复发起。 */
  const retry = useCallback(async () => {
    if (inFlightRequestRef.current !== undefined) return;
    await load(false);
  }, [load]);

  return { groups, transition, error, retry };
}

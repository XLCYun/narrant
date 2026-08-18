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

  /** 加载当前 Repository，并只允许最新 intent 发布结果。 */
  const load = useCallback(async (clear: boolean) => {
    if (!options.enabled || options.repositoryId === undefined) return;
    const request = ++requestRef.current;
    if (clear) {
      groupsRef.current = [];
      setGroups([]);
    }
    setError(undefined);
    setTransition({ kind: clear || groupsRef.current.length === 0 ? "loading" : "refreshing" });
    try {
      const discovered = await adapter.discover(options.repositoryId);
      if (request !== requestRef.current) return;
      groupsRef.current = discovered;
      setGroups(discovered);
    } catch (reason) {
      if (request !== requestRef.current) return;
      setError(applicationError("pull-request-discovery", "discover", reason));
    } finally {
      if (request === requestRef.current) setTransition({ kind: "stable" });
    }
  }, [adapter, options.enabled, options.repositoryId]);

  useEffect(() => {
    const repositoryChanged = repositoryRef.current !== options.repositoryId;
    repositoryRef.current = options.repositoryId;
    if (!options.enabled || options.repositoryId === undefined) {
      requestRef.current += 1;
      groupsRef.current = [];
      setGroups([]);
      setError(undefined);
      setTransition({ kind: "idle" });
      return;
    }
    void load(repositoryChanged);
    return () => {
      requestRef.current += 1;
    };
  }, [load, options.enabled, options.repositoryId, options.signedIn]);

  return { groups, transition, error, retry: () => load(false) };
}

import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { PullRequestDiscoveryApplicationAdapter } from "@/github/pull-request-discovery-application";
import { usePullRequestDiscoveryLifecycle } from "@/github/use-pull-request-discovery-lifecycle";
import type { PullRequestGroup } from "@/lib/review-types";

/** 创建可由测试显式完成或拒绝的异步结果。 */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((complete, fail) => {
    resolve = complete;
    reject = fail;
  });
  return { promise, resolve, reject };
}

const publicGroups: PullRequestGroup[] = [{
  repository: { owner: "octo-org", name: "public-repo" },
  remoteNames: ["origin"],
  pullRequests: [],
  error: null,
}];

const otherGroups: PullRequestGroup[] = [{
  repository: { owner: "octo-org", name: "other-repo" },
  remoteNames: ["upstream"],
  pullRequests: [],
  error: null,
}];

describe("Pull Request Discovery lifecycle", () => {
  it("首次加载和手动刷新均忽略重复触发，失败后可重试当前 Repository", async () => {
    const initial = deferred<PullRequestGroup[]>();
    const refreshing = deferred<PullRequestGroup[]>();
    const discover = vi.fn().mockReturnValueOnce(initial.promise).mockReturnValueOnce(refreshing.promise).mockResolvedValue(otherGroups);
    const adapter = { discover };
    const { result } = renderHook(() => usePullRequestDiscoveryLifecycle(adapter, {
      enabled: true, repositoryId: 17, signedIn: false,
    }));
    await act(async () => { await result.current.retry(); });
    expect(discover).toHaveBeenCalledTimes(1);
    await act(async () => initial.resolve(publicGroups));

    let refresh!: Promise<void>;
    act(() => {
      refresh = result.current.retry();
      void result.current.retry();
    });
    expect(discover).toHaveBeenCalledTimes(2);
    expect(result.current.transition.kind).toBe("refreshing");
    expect(result.current.groups).toEqual(publicGroups);
    await act(async () => { refreshing.reject(new Error("offline")); await refresh; });
    expect(result.current.groups).toEqual(publicGroups);
    expect(result.current.error?.detail).toContain("offline");
    await act(async () => { await result.current.retry(); });
    expect(discover.mock.calls).toEqual([[17], [17], [17]]);
    expect(result.current.groups).toEqual(otherGroups);
    expect(result.current.error).toBeUndefined();
    expect(result.current.transition.kind).toBe("stable");
  });

  it("逐仓库更新成功结果、保留失败条目，成功空结果和移除的来源不保留旧条目", async () => {
    const pullRequest = {
      number: 1, title: "Available before refresh", description: "", authorUsername: "octocat",
      state: "open", isDraft: false, webUrl: "https://github.com/octo-org/public-repo/pull/1",
      githubUpdatedAt: "2026-10-18T08:00:00Z", base: { ref: "main", sha: "base" }, head: { ref: "topic", sha: "head" },
    };
    const cached = { ...publicGroups[0], pullRequests: [pullRequest] };
    const removed = { ...otherGroups[0], repository: { owner: "removed", name: "remote" }, pullRequests: [pullRequest] };
    const failed = { ...publicGroups[0], repository: { owner: "OCTO-ORG", name: "PUBLIC-REPO" }, error: "rate limited" };
    const updated = { ...otherGroups[0], pullRequests: [{ ...pullRequest, title: "Latest result" }] };
    const discover = vi.fn()
      .mockResolvedValueOnce([cached, otherGroups[0], removed])
      .mockResolvedValueOnce([failed, updated])
      .mockResolvedValueOnce(publicGroups);
    const adapter = { discover };
    const { result } = renderHook(() => usePullRequestDiscoveryLifecycle(adapter, {
      enabled: true, repositoryId: 17, signedIn: true,
    }));
    await waitFor(() => expect(result.current.transition.kind).toBe("stable"));
    await act(async () => { await result.current.retry(); });
    expect(result.current.groups).toEqual([{ ...failed, pullRequests: [pullRequest] }, updated]);
    await act(async () => { await result.current.retry(); });
    expect(result.current.groups).toEqual(publicGroups);
  });

  it("GitHub Identity 变化时保留现有 discovery，刷新失败只产生局部错误", async () => {
    const requests: Array<ReturnType<typeof deferred<PullRequestGroup[]>>> = [];
    const adapter: PullRequestDiscoveryApplicationAdapter = {
      discover: () => {
        const request = deferred<PullRequestGroup[]>();
        requests.push(request);
        return request.promise;
      },
    };
    const { result, rerender } = renderHook(
      ({ signedIn }) => usePullRequestDiscoveryLifecycle(adapter, {
        enabled: true,
        repositoryId: 17,
        signedIn,
      }),
      { initialProps: { signedIn: false } },
    );
    await waitFor(() => expect(requests).toHaveLength(1));

    act(() => requests[0].resolve(publicGroups));
    await waitFor(() => expect(result.current.groups).toEqual(publicGroups));

    rerender({ signedIn: true });
    await waitFor(() => expect(requests).toHaveLength(2));
    expect(result.current.groups).toEqual(publicGroups);
    expect(result.current.transition.kind).toBe("refreshing");

    act(() => requests[1].reject(new Error("private discovery unavailable")));
    await waitFor(() => expect(result.current.error).toEqual({
      scope: "pull-request-discovery",
      operation: "discover",
      detail: "Error: private discovery unavailable",
    }));
    expect(result.current.groups).toEqual(publicGroups);

    let retrying!: Promise<void>;
    act(() => {
      retrying = result.current.retry();
    });
    await waitFor(() => expect(requests).toHaveLength(3));
    expect(result.current.transition.kind).toBe("refreshing");
    act(() => requests[2].resolve(otherGroups));
    await retrying;
    await waitFor(() => expect(result.current.groups).toEqual(otherGroups));
  });

  it("Repository 变化时清空旧结果且不发布迟到 discovery", async () => {
    const requests = new Map<number, ReturnType<typeof deferred<PullRequestGroup[]>>>();
    const adapter: PullRequestDiscoveryApplicationAdapter = {
      discover: (repositoryId) => {
        const request = deferred<PullRequestGroup[]>();
        requests.set(repositoryId, request);
        return request.promise;
      },
    };
    const { result, rerender } = renderHook(
      ({ repositoryId }) => usePullRequestDiscoveryLifecycle(adapter, {
        enabled: true,
        repositoryId,
        signedIn: false,
      }),
      { initialProps: { repositoryId: 17 } },
    );
    await waitFor(() => expect(requests.has(17)).toBe(true));

    rerender({ repositoryId: 23 });
    await waitFor(() => expect(requests.has(23)).toBe(true));
    expect(result.current.groups).toEqual([]);

    act(() => requests.get(17)?.resolve(publicGroups));
    await Promise.resolve();
    expect(result.current.groups).toEqual([]);

    act(() => requests.get(23)?.resolve(otherGroups));
    await waitFor(() => expect(result.current.groups).toEqual(otherGroups));
  });
});

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
  remoteNames: ["origin"], page: 1, nextPage: null,
  pullRequests: [],
  error: null,
}];

const otherGroups: PullRequestGroup[] = [{
  repository: { owner: "octo-org", name: "other-repo" },
  remoteNames: ["upstream"], page: 1, nextPage: null,
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
    await act(async () => { await result.current.refresh(); });
    expect(discover).toHaveBeenCalledTimes(1);
    await act(async () => initial.resolve(publicGroups));

    let refresh!: Promise<void>;
    act(() => {
      refresh = result.current.refresh();
      void result.current.refresh();
    });
    expect(discover).toHaveBeenCalledTimes(2);
    expect(result.current.transition.kind).toBe("refreshing");
    expect(result.current.groups).toEqual(publicGroups);
    await act(async () => { refreshing.reject(new Error("offline")); await refresh; });
    expect(result.current.groups).toEqual(publicGroups);
    expect(result.current.error?.detail).toContain("offline");
    await act(async () => { await result.current.refresh(); });
    expect(discover.mock.calls).toEqual([[17, { state: "open" }], [17, { state: "open" }], [17, { state: "open" }]]);
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
    await act(async () => { await result.current.refresh(); });
    expect(result.current.groups).toEqual([{ ...failed, pullRequests: [pullRequest] }, updated]);
    await act(async () => { await result.current.refresh(); });
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
      retrying = result.current.refresh();
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

/** 每个成功页都携带独立的后续位置，空条目不代表末页。 */
function group(page: number, nextPage: number | null, numbers: number[], name = "public-repo"): PullRequestGroup {
  return {
    repository: { owner: "octo-org", name }, remoteNames: [name], page, nextPage, error: null,
    pullRequests: numbers.map((number) => ({
      number, title: `PR ${number}`, state: "open", isDraft: false, description: "", authorUsername: "octocat",
      webUrl: `https://github.com/octo-org/${name}/pull/${number}`, githubUpdatedAt: "2026-10-18T00:00:00Z",
      base: { ref: "main", sha: "base" }, head: { ref: "topic", sha: "head" },
    })),
  };
}

const lifecycleOptions = { enabled: true, repositoryId: 17, signedIn: false };

describe("有界 Discovery 查询与分页", () => {
  it("Open 首页只调用一次，加载更多去重且保留较新条目，到末尾不再请求；刷新重置页码", async () => {
    const more = deferred<PullRequestGroup[]>();
    const first = group(1, 2, [1]);
    first.pullRequests[0].title = "Latest title";
    first.pullRequests[0].githubUpdatedAt = "2026-10-19T00:00:00Z";
    const discover = vi.fn().mockResolvedValueOnce([first]).mockReturnValueOnce(more.promise).mockResolvedValueOnce([group(1, 2, [3])]);
    const adapter = { discover };
    const { result } = renderHook(() => usePullRequestDiscoveryLifecycle(adapter, lifecycleOptions));
    await waitFor(() => expect(result.current.transition.kind).toBe("stable"));
    expect(discover.mock.calls).toEqual([[17, { state: "open" }]]);
    expect(result.current.complete).toBe(false);
    let pending!: Promise<void>;
    act(() => {
      pending = result.current.loadMore();
      void result.current.loadMore();
      void result.current.refresh();
      void result.current.retry();
    });
    expect(result.current.transition.kind).toBe("loading-more");
    expect(result.current.groups[0].pullRequests).toEqual(first.pullRequests);
    expect(discover).toHaveBeenCalledTimes(2);
    expect(discover.mock.calls[1]).toEqual([17, { state: "open", pages: [{ repository: first.repository, page: 2 }] }]);
    await act(async () => { more.resolve([group(2, null, [1, 2])]); await pending; });
    expect(result.current.groups[0].pullRequests.map((pr) => pr.number)).toEqual([1, 2]);
    expect(result.current.groups[0].pullRequests[0].title).toBe("Latest title");
    expect(result.current.complete).toBe(true);
    await act(async () => { await result.current.loadMore(); });
    expect(discover).toHaveBeenCalledTimes(2);
    await act(async () => { await result.current.refresh(); });
    expect(discover.mock.calls[2]).toEqual([17, { state: "open" }]);
    expect(result.current.groups[0]).toEqual(group(1, 2, [3]));
    expect(result.current.complete).toBe(false);
  });

  it("分页失败保留条目与原页，可重试且不会误报末尾", async () => {
    const first = group(1, 2, [1]);
    const failed = group(2, 2, []);
    failed.error = "rate limited";
    const discover = vi.fn().mockResolvedValueOnce([first]).mockResolvedValueOnce([failed])
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce([group(2, null, [2])]);
    const adapter = { discover };
    const { result } = renderHook(() => usePullRequestDiscoveryLifecycle(adapter, lifecycleOptions));
    await waitFor(() => expect(result.current.transition.kind).toBe("stable"));
    await act(async () => { await result.current.loadMore(); });
    expect(result.current.groups[0].pullRequests).toEqual(first.pullRequests);
    expect(result.current.groups[0].error).toBe("rate limited");
    expect(result.current.complete).toBe(false);
    await act(async () => { await result.current.retry(); });
    expect(result.current.error?.detail).toContain("offline");
    expect(result.current.groups[0].nextPage).toBe(2);
    await act(async () => { await result.current.retry(); });
    expect(result.current.error).toBeUndefined();
    expect(result.current.groups[0].pullRequests.map((pr) => pr.number)).toEqual([1, 2]);
    expect(result.current.complete).toBe(true);
    expect(discover.mock.calls.slice(1)).toEqual(Array(3).fill([17, { state: "open", pages: [{ repository: first.repository, page: 2 }] }]));
  });

  it("筛选请求与分页隔离，未匹配的扫描页仍能加载，迟到首页和后续页均不污染新查询", async () => {
    const requests: Array<ReturnType<typeof deferred<PullRequestGroup[]>>> = [];
    const discover = vi.fn(() => { const request = deferred<PullRequestGroup[]>(); requests.push(request); return request.promise; });
    const adapter = { discover };
    const { result } = renderHook(() => usePullRequestDiscoveryLifecycle(adapter, lifecycleOptions));
    expect(result.current.complete).toBe(false);
    act(() => result.current.selectFilter("closed"));
    expect(discover.mock.calls).toHaveLength(2);
    const scanned = group(1, 2, [42]);
    scanned.pullRequests[0].state = "merged";
    await act(async () => requests[1].resolve([scanned]));
    expect(result.current.filter).toBe("closed");
    expect(result.current.complete).toBe(false);
    await act(async () => requests[0].resolve([group(1, null, [1])]));
    expect(result.current.groups).toEqual([scanned]);
    act(() => result.current.selectFilter("merged"));
    expect(result.current.groups).toEqual([]);
    expect(result.current.complete).toBe(false);
    await act(async () => requests[2].resolve([scanned]));
    act(() => result.current.selectFilter("closed"));
    expect(discover).toHaveBeenCalledTimes(3);
    expect(result.current.groups).toEqual([scanned]);
    act(() => { void result.current.loadMore(); });
    act(() => result.current.selectFilter("all"));
    await act(async () => requests[4].resolve([group(1, null, [10])]));
    await act(async () => requests[3].resolve([group(2, null, [99])]));
    expect(result.current.groups[0].pullRequests[0].number).toBe(10);
    act(() => result.current.selectFilter("closed"));
    expect(result.current.groups).toEqual([scanned]);
    act(() => { void result.current.loadMore(); });
    const closed = group(2, null, [99]);
    closed.pullRequests[0].state = "closed";
    await act(async () => requests[5].resolve([closed]));
    expect(result.current.groups[0].pullRequests.map((pr) => pr.state)).toEqual(["merged", "closed"]);
    expect(result.current.complete).toBe(true);
    expect(discover.mock.calls).toEqual([
      [17, { state: "open" }], [17, { state: "closed" }], [17, { state: "merged" }],
      [17, { state: "closed", pages: [{ repository: scanned.repository, page: 2 }] }],
      [17, { state: "all" }], [17, { state: "closed", pages: [{ repository: scanned.repository, page: 2 }] }],
    ]);
  });

  it("多来源失败独立：正常来源可继续，重试只恢复失败页；刷新失败重试首页会替换旧后续页", async () => {
    const publicFirst = group(1, 2, [1]);
    const privateFirst = group(1, 2, [2], "private");
    const failedRefresh = { ...group(1, 1, [], "private"), error: "offline" };
    const discover = vi.fn().mockResolvedValueOnce([publicFirst, privateFirst])
      .mockResolvedValueOnce([group(2, null, [11]), group(2, null, [22], "private")])
      .mockResolvedValueOnce([group(1, 2, [3]), failedRefresh])
      .mockResolvedValueOnce([group(2, null, [33])])
      .mockResolvedValueOnce([group(1, 2, [4], "private")]);
    const adapter = { discover };
    const { result } = renderHook(() => usePullRequestDiscoveryLifecycle(adapter, lifecycleOptions));
    await waitFor(() => expect(result.current.transition.kind).toBe("stable"));
    await act(async () => { await result.current.loadMore(); });
    expect(result.current.complete).toBe(true);
    await act(async () => { await result.current.refresh(); });
    expect(result.current.groups[1].pullRequests.map((pr) => pr.number)).toEqual([2, 22]);
    expect(result.current.groups[1].nextPage).toBe(1);
    await act(async () => { await result.current.loadMore(); });
    expect(discover.mock.calls[3]).toEqual([17, { state: "open", pages: [{ repository: publicFirst.repository, page: 2 }] }]);
    await act(async () => { await result.current.retry(); });
    expect(discover.mock.calls[4]).toEqual([17, { state: "open", pages: [{ repository: privateFirst.repository, page: 1 }] }]);
    const restored = result.current.groups.find((value) => value.repository.name === "private")!;
    expect(restored.pullRequests.map((pr) => pr.number)).toEqual([4]);
    expect(restored.nextPage).toBe(2);
    expect(result.current.groups.find((value) => value.repository.name === "public-repo")!.pullRequests.map((pr) => pr.number)).toEqual([3, 33]);
  });

  it("离开概览清除查询，返回默认 Open；身份变化保留已加载内容并使其他筛选重取首页", async () => {
    const discover = vi.fn().mockResolvedValue([group(1, 2, [1])]);
    const adapter = { discover };
    const { result, rerender } = renderHook((options) => usePullRequestDiscoveryLifecycle(adapter, options), { initialProps: lifecycleOptions });
    await waitFor(() => expect(result.current.transition.kind).toBe("stable"));
    act(() => result.current.selectFilter("all"));
    await waitFor(() => expect(result.current.transition.kind).toBe("stable"));
    rerender({ ...lifecycleOptions, signedIn: true });
    await waitFor(() => expect(discover).toHaveBeenCalledTimes(3));
    expect(result.current.filter).toBe("all");
    act(() => result.current.selectFilter("open"));
    await waitFor(() => expect(discover).toHaveBeenCalledTimes(4));
    rerender({ ...lifecycleOptions, enabled: false });
    expect(result.current.groups).toEqual([]);
    expect(result.current.transition.kind).toBe("idle");
    rerender(lifecycleOptions);
    await waitFor(() => expect(discover).toHaveBeenCalledTimes(5));
    expect(result.current.filter).toBe("open");
  });
});

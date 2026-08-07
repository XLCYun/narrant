import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";

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
        repositoryEntryId: 17,
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

  it("Repository Entry 变化时清空旧结果且不发布迟到 discovery", async () => {
    const requests = new Map<number, ReturnType<typeof deferred<PullRequestGroup[]>>>();
    const adapter: PullRequestDiscoveryApplicationAdapter = {
      discover: (repositoryEntryId) => {
        const request = deferred<PullRequestGroup[]>();
        requests.set(repositoryEntryId, request);
        return request.promise;
      },
    };
    const { result, rerender } = renderHook(
      ({ repositoryEntryId }) => usePullRequestDiscoveryLifecycle(adapter, {
        enabled: true,
        repositoryEntryId,
        signedIn: false,
      }),
      { initialProps: { repositoryEntryId: 17 } },
    );
    await waitFor(() => expect(requests.has(17)).toBe(true));

    rerender({ repositoryEntryId: 23 });
    await waitFor(() => expect(requests.has(23)).toBe(true));
    expect(result.current.groups).toEqual([]);

    act(() => requests.get(17)?.resolve(publicGroups));
    await Promise.resolve();
    expect(result.current.groups).toEqual([]);

    act(() => requests.get(23)?.resolve(otherGroups));
    await waitFor(() => expect(result.current.groups).toEqual(otherGroups));
  });
});

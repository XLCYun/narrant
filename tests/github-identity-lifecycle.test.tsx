import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type {
  GitHubAuthStatus,
  GitHubOAuthApplicationAdapter,
} from "@/github/github-oauth-application";
import {
  useGitHubIdentityLifecycle,
  type GitHubIdentityLifecycleBinding,
} from "@/github/use-github-identity-lifecycle";

/** 创建可由测试显式完成的异步结果。 */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

const cachedStatus: GitHubAuthStatus = {
  configured: true,
  signedIn: true,
  appInstallUrl: null,
  identity: { id: 1, login: "cached", name: null, avatarPath: null },
};

describe("GitHub Identity lifecycle", () => {
  it("先发布 cached GitHub Identity，再在后台刷新公开身份", async () => {
    const status = deferred<GitHubAuthStatus>();
    const refreshedIdentity = deferred<NonNullable<GitHubAuthStatus["identity"]>>();
    const adapter: GitHubOAuthApplicationAdapter = {
      loadStatus: () => status.promise,
      refreshIdentity: () => refreshedIdentity.promise,
      beginAuthorization: () => Promise.reject(new Error("unused")),
      completeAuthorization: () => Promise.reject(new Error("unused")),
      signOut: () => Promise.reject(new Error("unused")),
    };

    const { result } = renderHook(() => useGitHubIdentityLifecycle(adapter, { enabled: true }));

    expect(result.current.transition.kind).toBe("loading");

    act(() => status.resolve(cachedStatus));
    await waitFor(() => expect(result.current.status).toEqual(cachedStatus));
    expect(result.current.transition.kind).toBe("refreshing-identity");

    const refreshed = { id: 1, login: "octocat", name: "Octo Cat", avatarPath: "/avatar" };
    act(() => refreshedIdentity.resolve(refreshed));
    await waitFor(() => expect(result.current.status?.identity).toEqual(refreshed));
    expect(result.current.transition.kind).toBe("stable");
  });

  it("完成 Device Flow 后立即提交新的 GitHub Identity 状态", async () => {
    const authorization = {
      authorizationId: "authorization-1",
      userCode: "ABCD-EFGH",
      verificationUri: "https://github.com/login/device",
      expiresIn: 900,
      interval: 5,
    };
    const signedOut = { ...cachedStatus, signedIn: false, identity: null };
    const signedIn = {
      ...cachedStatus,
      identity: { id: 2, login: "new-user", name: null, avatarPath: null },
    };
    const completion = deferred<GitHubAuthStatus>();
    const adapter: GitHubOAuthApplicationAdapter = {
      loadStatus: () => Promise.resolve(signedOut),
      refreshIdentity: () => Promise.reject(new Error("unused")),
      beginAuthorization: () => Promise.resolve(authorization),
      completeAuthorization: () => completion.promise,
      signOut: () => Promise.reject(new Error("unused")),
    };
    const { result } = renderHook(() => useGitHubIdentityLifecycle(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.transition.kind).toBe("stable"));

    await act(async () => {
      await result.current.beginAuthorization();
    });
    expect(result.current.authorization).toEqual(authorization);

    let completing!: Promise<{ kind: "authorized" } | { kind: "failed" }>;
    act(() => {
      completing = result.current.completeAuthorization();
    });
    await waitFor(() => expect(result.current.transition.kind).toBe("completing-authorization"));

    act(() => completion.resolve(signedIn));
    await expect(completing).resolves.toEqual({ kind: "authorized" });
    await waitFor(() => expect(result.current.status).toEqual(signedIn));
    expect(result.current.authorization).toBeUndefined();
    expect(result.current.transition.kind).toBe("stable");
  });

  it("退出失败时保留有效身份并在 module 内提供可恢复错误", async () => {
    const adapter: GitHubOAuthApplicationAdapter = {
      loadStatus: () => Promise.resolve(cachedStatus),
      refreshIdentity: () => Promise.reject(new Error("offline")),
      beginAuthorization: () => Promise.reject(new Error("unused")),
      completeAuthorization: () => Promise.reject(new Error("unused")),
      signOut: () => Promise.resolve({
        kind: "failed",
        detail: "credential store unavailable",
        status: cachedStatus,
      }),
    };
    const { result } = renderHook(() => useGitHubIdentityLifecycle(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.transition.kind).toBe("stable"));

    await act(async () => {
      await result.current.signOut();
    });

    expect(result.current.status).toEqual(cachedStatus);
    expect(result.current.error).toEqual({
      scope: "github-identity",
      operation: "sign-out",
      detail: "credential store unavailable",
    });
  });

  it("cached authorization status 读取失败后可通过同一 interface 重试", async () => {
    let attempts = 0;
    const signedOut = { ...cachedStatus, signedIn: false, identity: null };
    const adapter: GitHubOAuthApplicationAdapter = {
      loadStatus: () => attempts++ === 0
        ? Promise.reject(new Error("credential store unavailable"))
        : Promise.resolve(signedOut),
      refreshIdentity: () => Promise.reject(new Error("unused")),
      beginAuthorization: () => Promise.reject(new Error("unused")),
      completeAuthorization: () => Promise.reject(new Error("unused")),
      signOut: () => Promise.reject(new Error("unused")),
    };
    const { result } = renderHook(() => useGitHubIdentityLifecycle(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.error?.operation).toBe("load-status"));

    await act(async () => {
      await result.current.retry();
    });

    expect(result.current.status).toEqual(signedOut);
    expect(result.current.error).toBeUndefined();
    expect(result.current.transition.kind).toBe("stable");
  });

  it("较新的退出 intent 不会被迟到的授权完成结果覆盖", async () => {
    const authorization = {
      authorizationId: "authorization-1",
      userCode: "ABCD-EFGH",
      verificationUri: "https://github.com/login/device",
      expiresIn: 900,
      interval: 5,
    };
    const signedOut = { ...cachedStatus, signedIn: false, identity: null };
    const lateSignedIn = { ...cachedStatus, identity: { ...cachedStatus.identity!, login: "late-user" } };
    const completion = deferred<GitHubAuthStatus>();
    const adapter: GitHubOAuthApplicationAdapter = {
      loadStatus: () => Promise.resolve(signedOut),
      refreshIdentity: () => Promise.reject(new Error("unused")),
      beginAuthorization: () => Promise.resolve(authorization),
      completeAuthorization: () => completion.promise,
      signOut: () => Promise.resolve({ kind: "signed-out", status: signedOut }),
    };
    const { result } = renderHook(() => useGitHubIdentityLifecycle(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.transition.kind).toBe("stable"));
    await act(async () => {
      await result.current.beginAuthorization();
    });

    let completing!: Promise<{ kind: "authorized" } | { kind: "failed" }>;
    act(() => {
      completing = result.current.completeAuthorization();
    });
    await waitFor(() => expect(result.current.transition.kind).toBe("completing-authorization"));
    await act(async () => {
      await result.current.signOut();
    });
    expect(result.current.status).toEqual(signedOut);

    act(() => completion.resolve(lateSignedIn));
    await expect(completing).resolves.toEqual({ kind: "failed" });
    expect(result.current.status).toEqual(signedOut);
    expect(result.current.transition.kind).toBe("stable");
  });

  it("重复开始授权时只发布最后一次 intent 的 Device Flow", async () => {
    const signedOut = { ...cachedStatus, signedIn: false, identity: null };
    const first = deferred<Awaited<ReturnType<GitHubOAuthApplicationAdapter["beginAuthorization"]>>>();
    const second = deferred<Awaited<ReturnType<GitHubOAuthApplicationAdapter["beginAuthorization"]>>>();
    let attempts = 0;
    const adapter: GitHubOAuthApplicationAdapter = {
      loadStatus: () => Promise.resolve(signedOut),
      refreshIdentity: () => Promise.reject(new Error("unused")),
      beginAuthorization: () => attempts++ === 0 ? first.promise : second.promise,
      completeAuthorization: () => Promise.reject(new Error("unused")),
      signOut: () => Promise.reject(new Error("unused")),
    };
    const { result } = renderHook(() => useGitHubIdentityLifecycle(adapter, { enabled: true }));
    await waitFor(() => expect(result.current.transition.kind).toBe("stable"));

    let firstIntent!: ReturnType<GitHubIdentityLifecycleBinding["beginAuthorization"]>;
    let secondIntent!: ReturnType<GitHubIdentityLifecycleBinding["beginAuthorization"]>;
    act(() => {
      firstIntent = result.current.beginAuthorization();
      secondIntent = result.current.beginAuthorization();
    });
    const latestAuthorization = {
      authorizationId: "authorization-2",
      userCode: "IJKL-MNOP",
      verificationUri: "https://github.com/login/device",
      expiresIn: 900,
      interval: 5,
    };
    act(() => second.resolve(latestAuthorization));
    await expect(secondIntent).resolves.toEqual({ kind: "started", authorization: latestAuthorization });
    await waitFor(() => expect(result.current.authorization).toEqual(latestAuthorization));

    act(() => first.resolve({ ...latestAuthorization, authorizationId: "authorization-1" }));
    await expect(firstIntent).resolves.toEqual({ kind: "failed" });
    expect(result.current.authorization).toEqual(latestAuthorization);
    expect(result.current.transition.kind).toBe("stable");
  });
});

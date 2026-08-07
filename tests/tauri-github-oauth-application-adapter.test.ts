import { describe, expect, it, vi } from "vitest";

import { createTauriGitHubOAuthApplicationAdapter } from "@/infrastructure/tauri-github-oauth-application-adapter";
import type { TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type { GitHubAuthStatus } from "@/github/github-oauth-application";

const cachedStatus: GitHubAuthStatus = {
  configured: true,
  signedIn: true,
  appInstallUrl: null,
  identity: { id: 1, login: "cached", name: null, avatarPath: null },
};

describe("Tauri GitHub OAuth application adapter", () => {
  it("独立读取 cached authorization status 与远程 GitHub Identity", async () => {
    const refreshedIdentity = { id: 2, login: "octocat", name: "Octo Cat", avatarPath: "/avatar" };
    const invoke = vi.fn(async (command: string) => {
      if (command === "github_auth_status") return cachedStatus;
      if (command === "refresh_github_identity") return refreshedIdentity;
      throw new Error(`unexpected command: ${command}`);
    });
    const adapter = createTauriGitHubOAuthApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.loadStatus()).resolves.toEqual(cachedStatus);
    await expect(adapter.refreshIdentity()).resolves.toEqual(refreshedIdentity);
    expect(invoke).toHaveBeenNthCalledWith(1, "github_auth_status", {});
    expect(invoke).toHaveBeenNthCalledWith(2, "refresh_github_identity", {});
  });

  it("完成 device flow 后返回后端确认的最新授权状态", async () => {
    const refreshedIdentity = { id: 2, login: "octocat", name: "Octo Cat", avatarPath: "/avatar" };
    const signedIn = { ...cachedStatus, identity: refreshedIdentity };
    const invoke = vi.fn(async (command: string) => {
      if (command === "complete_github_authorization") return undefined;
      if (command === "github_auth_status") return signedIn;
      throw new Error(`unexpected command: ${command}`);
    });
    const adapter = createTauriGitHubOAuthApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.completeAuthorization("authorization-1")).resolves.toEqual(signedIn);
    expect(invoke).toHaveBeenNthCalledWith(1, "complete_github_authorization", {
      authorizationId: "authorization-1",
    });
  });

  it("登出失败时返回错误与后端仍然有效的授权状态", async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === "sign_out_github") throw new Error("credential store unavailable");
      if (command === "github_auth_status") return cachedStatus;
      throw new Error(`unexpected command: ${command}`);
    });
    const adapter = createTauriGitHubOAuthApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.signOut()).resolves.toEqual({
      kind: "failed",
      detail: "Error: credential store unavailable",
      status: cachedStatus,
    });
  });
});

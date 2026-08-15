import { describe, expect, it, vi } from "vitest";

import { createTauriRepositoryApplicationAdapter } from "@/infrastructure/tauri-repository-application-adapter";
import type { TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type { RepositorySummary } from "@/repository/repository-application";

const repository: RepositorySummary = {
  id: 17,
  canonicalPath: "/repo",
  gitCommonDirectory: "/repo/.git",
  createdAtMillis: 1,
  lastOpenedAtMillis: 2,
  availability: { kind: "available" },
};

describe("Tauri Repository application adapter", () => {
  it("一次加载 Repository Navigation 与全局最近 Reviews", async () => {
    const recentReviews = [{ reviewId: "review-1" }];
    const invoke = vi.fn(async (command: string) => {
      if (command === "load_repository_navigation") return { repositories: [repository], recentReviews };
      throw new Error(`unexpected command: ${command}`);
    });
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.loadNavigation()).resolves.toEqual({
      repositories: [repository],
      recentReviews,
    });
    expect(invoke).toHaveBeenCalledWith("load_repository_navigation", {});
  });

  it("打开未授权 Repository 时返回 trust-required 且不加载 Review 列表", async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === "open_repository_overview") return { kind: "trust-required", repository };
      throw new Error(`unexpected command: ${command}`);
    });
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.open(17)).resolves.toEqual({
      kind: "trust-required",
      repository,
    });
    expect(invoke).toHaveBeenCalledOnce();
  });

  it("不可用 Repository 不检查 Workspace Trust", async () => {
    const unavailable = { ...repository, availability: { kind: "unavailable", reason: "missing" } };
    const invoke = vi.fn().mockResolvedValue({ kind: "unavailable", repository: unavailable });
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.open(17)).resolves.toEqual({ kind: "unavailable", repository: unavailable });
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("授权后返回可直接进入的 Repository Overview", async () => {
    const reviews = [{ reviewId: "review-1", repositoryId: 17 }];
    const invoke = vi.fn(async (command: string) => {
      if (command === "grant_repository_workspace_trust_and_open") {
        return { repository, reviews };
      }
      throw new Error(`unexpected command: ${command}`);
    });
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.grantTrust(repository)).resolves.toEqual({
      repository,
      reviews,
    });
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "grant_repository_workspace_trust_and_open",
    ]);
  });

  it("注册 clone 并按 Repository identity 刷新 Review 列表", async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === "register_repository") return repository;
      if (command === "load_repository_reviews") return { reviews: [] };
      throw new Error(`unexpected command: ${command}`);
    });
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.register("/repo")).resolves.toBe(repository);
    await expect(adapter.loadReviews(17)).resolves.toEqual({ reviews: [] });
  });

  it("通过 cancellation interface 加载 Branch refs", async () => {
    const branches = { refs: ["main"], defaultRef: "main", currentRef: "main" };
    const invokeCancellable = vi.fn().mockResolvedValue(branches);
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke: vi.fn(),
      invokeCancellable,
    } as unknown as TypedTauriTransport);
    const signal = new AbortController().signal;

    await expect(adapter.loadBranches(17, signal)).resolves.toBe(branches);
    expect(invokeCancellable).toHaveBeenCalledWith(
      "list_local_refs",
      { repositoryId: 17 },
      signal,
    );
  });

  it("一次读取 Repository Review projection", async () => {
    const projection = { reviews: [] };
    const invoke = vi.fn().mockResolvedValue(projection);
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.loadReviews(17)).resolves.toBe(projection);
    expect(invoke).toHaveBeenCalledWith("load_repository_reviews", { repositoryId: 17 });
  });
});

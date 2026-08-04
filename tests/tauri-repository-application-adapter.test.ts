import { describe, expect, it, vi } from "vitest";

import { createTauriRepositoryApplicationAdapter } from "@/infrastructure/tauri-repository-application-adapter";
import type { TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type { RepositoryEntry } from "@/repository/repository-application";

const repository: RepositoryEntry = {
  id: 17,
  canonicalPath: "/repo",
  commonDirectory: "/repo/.git",
  createdAt: 1,
  lastOpenedAt: 2,
  available: true,
  unavailableReason: null,
};

describe("Tauri Repository application adapter", () => {
  it("一次加载 Repository Catalog 与全局最近 Reviews", async () => {
    const recentReviews = [{ reviewId: "review-1" }];
    const invoke = vi.fn(async (command: string) => {
      if (command === "list_repository_entries") return [repository];
      if (command === "list_reviews") return recentReviews;
      throw new Error(`unexpected command: ${command}`);
    });
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.loadCatalog()).resolves.toEqual({
      repositories: [repository],
      recentReviews,
    });
    expect(invoke).toHaveBeenCalledWith("list_reviews", { repositoryEntryId: null });
  });

  it("打开未授权 Repository 时返回 trust-required 且不加载 Review 列表", async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === "open_repository_entry") return repository;
      if (command === "repository_workspace_trust") return false;
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
    expect(invoke).not.toHaveBeenCalledWith("list_saved_reviews", expect.anything());
    expect(invoke).not.toHaveBeenCalledWith("list_reviews", expect.anything());
  });

  it("不可用 Repository 不检查 Workspace Trust", async () => {
    const unavailable = { ...repository, available: false, unavailableReason: "missing" };
    const invoke = vi.fn().mockResolvedValue(unavailable);
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.open(17)).resolves.toEqual({ kind: "unavailable", repository: unavailable });
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("授权后返回可直接进入的 Repository context", async () => {
    const savedReviews = [{ reviewId: "review-1" }];
    const reviews = [{ reviewId: "review-1", repositoryEntryId: 17 }];
    const invoke = vi.fn(async (command: string) => {
      if (command === "grant_repository_workspace_trust") return undefined;
      if (command === "list_saved_reviews") return savedReviews;
      if (command === "list_reviews") return reviews;
      throw new Error(`unexpected command: ${command}`);
    });
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.grantTrust(repository)).resolves.toEqual({
      repository,
      savedReviews,
      reviews,
    });
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "grant_repository_workspace_trust",
      "list_saved_reviews",
      "list_reviews",
    ]);
  });

  it("注册 clone 并按 Repository identity 刷新 Review 列表", async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === "register_repository_entry") return repository;
      if (command === "list_saved_reviews" || command === "list_reviews") return [];
      throw new Error(`unexpected command: ${command}`);
    });
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.register("/repo")).resolves.toBe(repository);
    await expect(adapter.loadReviews(17)).resolves.toEqual({ savedReviews: [], reviews: [] });
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
      { repositoryEntryId: 17 },
      signal,
    );
  });

  it("记录 Repository 最近打开时间而不重载 context", async () => {
    const invoke = vi.fn().mockResolvedValue(repository);
    const adapter = createTauriRepositoryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.markOpened(17)).resolves.toBe(repository);
    expect(invoke).toHaveBeenCalledWith("open_repository_entry", { id: 17 });
  });
});

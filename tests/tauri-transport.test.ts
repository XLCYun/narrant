import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";

import type { BabysitProgress } from "@/babysit/types";
import { TAURI_COMMAND_NAMES } from "@/infrastructure/tauri/command-contract";
import { createTauriTransport } from "@/infrastructure/tauri/transport";
import type { Channel } from "@tauri-apps/api/core";

describe("Typed Tauri transport", () => {
  it("覆盖 Rust invoke handler 注册的全部 command", () => {
    const backend = readFileSync("src-tauri/src/tauri_app/bootstrap.rs", "utf8");
    const registered = backend.match(/generate_handler!\[([\s\S]*?)\]\)/)?.[1]
      .split(",")
      .map((name) => name.trim().split("::").at(-1))
      .filter(Boolean)
      .sort();

    expect([...TAURI_COMMAND_NAMES].sort()).toEqual(registered);
  });

  it("把 typed command 与参数原样交给 Tauri invoke", async () => {
    const rawInvoke = vi.fn().mockResolvedValue({
      repositories: [],
      recentReviews: [],
    });
    const transport = createTauriTransport(rawInvoke);

    await expect(transport.invoke("load_repository_navigation", {})).resolves.toEqual({
      repositories: [],
      recentReviews: [],
    });

    expect(rawInvoke).toHaveBeenCalledWith("load_repository_navigation", {});
  });

  it("explain command transport 不会把 locale 注入 invoke payload", async () => {
    const rawInvoke = vi.fn().mockResolvedValue({ before: {}, after: {} });
    const transport = createTauriTransport(rawInvoke);

    await transport.invokeCancellable("explain_review_file_change", {
      reviewId: "review-1",
      snapshotId: "snapshot-1",
      path: "src/example.ts",
      detectedLanguage: "typescript",
      generationPolicy: "reuse_cached",
      progress: {} as Channel<BabysitProgress>,
    }, new AbortController().signal);

    const payload = rawInvoke.mock.calls[0]?.[1] as Record<string, unknown>;
    expect(payload).toMatchObject({
      reviewId: "review-1",
      snapshotId: "snapshot-1",
      path: "src/example.ts",
      detectedLanguage: "typescript",
      generationPolicy: "reuse_cached",
    });
    expect(payload).not.toHaveProperty("locale");
  });

  it("为可取消 command 分配 operation identity 并在 abort 时通知后端", async () => {
    let resolveCommand!: (value: { refs: string[]; defaultRef: string; currentRef: string }) => void;
    const rawInvoke = vi.fn((command: string) => command === "list_local_refs"
      ? new Promise((resolve) => { resolveCommand = resolve; })
      : Promise.resolve(undefined));
    const transport = createTauriTransport(rawInvoke);
    const abort = new AbortController();

    const result = transport.invokeCancellable("list_local_refs", {
      repositoryId: 17,
    }, abort.signal);
    abort.abort();
    resolveCommand({ refs: ["refs/heads/main"], defaultRef: "refs/heads/main", currentRef: "refs/heads/main" });
    await result;

    const operationId = rawInvoke.mock.calls[0][1].operationId;
    expect(operationId).toMatch(/^operation-/);
    expect(rawInvoke).toHaveBeenNthCalledWith(2, "cancel_operation", { operationId });
  });

  it("不同 transport 实例不会为并发操作分配相同 identity", async () => {
    const firstInvoke = vi.fn(() => new Promise(() => undefined));
    const secondInvoke = vi.fn(() => new Promise(() => undefined));
    const first = createTauriTransport(firstInvoke);
    const second = createTauriTransport(secondInvoke);

    void first.invokeCancellable(
      "list_local_refs",
      { repositoryId: 1 },
      new AbortController().signal,
    );
    void second.invokeCancellable(
      "list_local_refs",
      { repositoryId: 1 },
      new AbortController().signal,
    );

    expect(firstInvoke.mock.calls[0][1].operationId)
      .not.toBe(secondInvoke.mock.calls[0][1].operationId);
  });

  it("不启动已经取消的 command", async () => {
    const rawInvoke = vi.fn();
    const transport = createTauriTransport(rawInvoke);
    const abort = new AbortController();
    abort.abort();

    await expect(transport.invokeCancellable("list_local_refs", {
      repositoryId: 17,
    }, abort.signal)).rejects.toMatchObject({ name: "AbortError" });
    expect(rawInvoke).not.toHaveBeenCalled();
  });
});

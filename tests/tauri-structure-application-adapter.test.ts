import type { Channel } from "@tauri-apps/api/core";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createTauriStructureApplicationAdapter } from "@/infrastructure/tauri-structure-application-adapter";
import type { StructureEvent, StructurePeekContent, StructureResult } from "@/review/structure-types";

const identity = { reviewId: "review-7", snapshotId: "snapshot-fixed" };

/** 空仓库仍有明确的覆盖结论，用于检查应用边界是否保留完整分析结果。 */
function completedResult(): StructureResult {
  return {
    ...identity,
    status: "completed",
    nodes: [],
    implementationGroups: [],
    relationships: [],
    unresolvedRelationships: [],
    sources: [],
    coverage: {
      discovery: { before: "complete", after: "complete" },
      files: [],
      diagnostics: [],
    },
    summary: {
      changedDeclarations: 0,
      relationshipChanges: 0,
      incompleteDeclarations: 0,
      unresolvedOccurrences: 0,
      complete: true,
    },
    initialFocus: { nodeIds: [], relationshipIds: [] },
  };
}

afterEach(() => clearMocks());

describe("Tauri Structure application adapter", () => {
  it("为固定 Snapshot 传递分析进度、检查点和最终结果", async () => {
    const result = completedResult();
    const events: StructureEvent[] = [
      { kind: "progress", progress: { phase: "discovery", side: "before", processedFiles: 0, totalFiles: null } },
      { kind: "checkpoint", result: { ...result, status: "running" } },
    ];
    const ipc = vi.fn((_command: string, payload: unknown) => {
      const progress = (payload as { progress: Channel<StructureEvent> }).progress;
      for (const event of events) progress.onmessage(event);
      return result;
    });
    mockIPC(ipc);
    const observed: StructureEvent[] = [];

    await expect(createTauriStructureApplicationAdapter().analyze(
      identity,
      (event) => observed.push(event),
      new AbortController().signal,
    )).resolves.toEqual(result);

    expect(observed).toEqual(events);
    expect(ipc).toHaveBeenCalledWith("analyze_review_structure", {
      reviewId: "review-7",
      snapshotId: "snapshot-fixed",
      operationId: expect.stringMatching(/^operation-/),
      progress: expect.objectContaining({ onmessage: expect.any(Function) }),
    });
  });

  it("按旧侧真实路径读取完整重命名文件对，保留另一侧不可用而非空文本", async () => {
    const content: StructurePeekContent = {
      ...identity,
      sourceId: "source-renamed",
      source: {
        id: "source-renamed",
        before: { state: "present", path: "src/原文件.ts", revision: "base-object" },
        after: { state: "present", path: "src/renamed.ts", revision: "head-object" },
        classification: {
          kind: "file-change",
          fileChange: { path: "src/renamed.ts", oldPath: "src/原文件.ts", status: "renamed" },
        },
      },
      before: {
        state: "available", path: "src/原文件.ts", revision: "base-object",
        objectId: "blob-original", text: "// entire file\nexport class Order {}\n", size: 37,
      },
      after: {
        state: "unavailable", reason: "git-object-unavailable",
        message: "Snapshot object is unavailable", retryable: true,
      },
    };
    const ipc = vi.fn(() => content);
    mockIPC(ipc);

    await expect(createTauriStructureApplicationAdapter().readSource(
      identity,
      { side: "before", path: "src/原文件.ts" },
      new AbortController().signal,
    )).resolves.toEqual(content);

    expect(ipc).toHaveBeenCalledWith("read_review_structure_source", {
      reviewId: "review-7", snapshotId: "snapshot-fixed",
      side: "before", path: "src/原文件.ts",
      operationId: expect.stringMatching(/^operation-/),
    });
  });

  it.each(["analyze", "readSource"] as const)("取消 %s 时通知相同 operation，保留后端失败原因", async (method) => {
    let reject!: (reason: unknown) => void;
    const pending = new Promise<never>((_resolve, rejectPromise) => { reject = rejectPromise; });
    const ipc = vi.fn<(command: string, payload: unknown) => Promise<never> | undefined>(
      (command) => command === "cancel_operation" ? undefined : pending,
    );
    mockIPC(ipc);
    const abort = new AbortController();
    const adapter = createTauriStructureApplicationAdapter();
    const operation = method === "analyze"
      ? adapter.analyze(identity, () => undefined, abort.signal)
      : adapter.readSource(identity, { side: "after", path: "src/order.rs" }, abort.signal);
    const failure = { code: "operation-cancelled", detail: "Analysis was cancelled" };
    const rejected = expect(operation).rejects.toEqual(failure);

    abort.abort();
    reject(failure);
    await rejected;

    const operationId = (ipc.mock.calls[0][1] as { operationId: string }).operationId;
    expect(operationId).toMatch(/^operation-/);
    expect(ipc).toHaveBeenCalledWith("cancel_operation", { operationId });
  });

  it.each(["analyze", "readSource"] as const)("不发送已取消的 %s 请求", async (method) => {
    const ipc = vi.fn();
    mockIPC(ipc);
    const abort = new AbortController();
    abort.abort();
    const adapter = createTauriStructureApplicationAdapter();
    const operation = method === "analyze"
      ? adapter.analyze(identity, () => undefined, abort.signal)
      : adapter.readSource(identity, { side: "after", path: "src/order.rs" }, abort.signal);

    await expect(operation).rejects.toMatchObject({ name: "AbortError" });
    expect(ipc).not.toHaveBeenCalled();
  });
});

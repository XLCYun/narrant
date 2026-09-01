import { describe, expect, it, vi } from "vitest";

import type { ApplicationUpdateSnapshot } from "@/application-update/application-update";
import { APPLICATION_UPDATE_SNAPSHOT_EVENT, createTauriApplicationUpdateAdapter } from "@/infrastructure/tauri-application-update-adapter";
import type { TypedTauriTransport } from "@/infrastructure/tauri/transport";

const value: ApplicationUpdateSnapshot = {
  sequence: 1, currentVersion: "2.0.0", automaticDownload: true,
  state: { kind: "idle" }, candidate: null, failure: null, recoveryWarning: null, preferenceWarning: null,
};

describe("Tauri Application Update adapter", () => {
  it("只监听唯一 snapshot event，并只发送五种窄命令参数", async () => {
    const invoke = vi.fn(async () => value);
    const transport = { invoke } as unknown as TypedTauriTransport;
    let eventHandler: ((event: { event: string; id: number; payload: ApplicationUpdateSnapshot }) => void) | undefined;
    const stop = vi.fn();
    const listen = vi.fn(async (_event, handler) => { eventHandler = handler; return stop; });
    const listener = vi.fn();
    const adapter = createTauriApplicationUpdateAdapter(transport, listen);

    const unsubscribe = await adapter.subscribe(listener);
    eventHandler?.({ event: APPLICATION_UPDATE_SNAPSHOT_EVENT, id: 1, payload: value });
    await adapter.snapshot();
    await adapter.manualCheck();
    await adapter.setAutomaticDownload(false);
    await adapter.retry();
    await adapter.restart();
    unsubscribe();

    expect(listen).toHaveBeenCalledWith(APPLICATION_UPDATE_SNAPSHOT_EVENT, expect.any(Function));
    expect(listener).toHaveBeenCalledWith(value);
    expect(invoke.mock.calls).toEqual([
      ["application_update_snapshot", {}],
      ["application_update_manual_check", {}],
      ["application_update_set_automatic_download", { enabled: false }],
      ["application_update_retry", {}],
      ["application_update_restart", {}],
    ]);
    expect(stop).toHaveBeenCalledOnce();
  });
});

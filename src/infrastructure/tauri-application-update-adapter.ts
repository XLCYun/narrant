import { listen, type Event } from "@tauri-apps/api/event";

import type { ApplicationUpdateApplication, ApplicationUpdateSnapshot } from "@/application-update/application-update";
import type { TypedTauriTransport } from "@/infrastructure/tauri/transport";

export const APPLICATION_UPDATE_SNAPSHOT_EVENT = "application-update:snapshot";

type SnapshotListener = (
  event: string,
  handler: (event: Event<ApplicationUpdateSnapshot>) => void,
) => Promise<() => void>;

/** 将 Rust 的五个窄命令和唯一 snapshot event 适配为前端更新端口。 */
export function createTauriApplicationUpdateAdapter(
  transport: TypedTauriTransport,
  listenForSnapshot: SnapshotListener = listen,
): ApplicationUpdateApplication {
  return {
    snapshot: () => transport.invoke("application_update_snapshot", {}),
    subscribe: (listener) => listenForSnapshot(
      APPLICATION_UPDATE_SNAPSHOT_EVENT,
      (event) => listener(event.payload),
    ),
    manualCheck: () => transport.invoke("application_update_manual_check", {}),
    setAutomaticDownload: (enabled) => transport.invoke("application_update_set_automatic_download", { enabled }),
    retry: () => transport.invoke("application_update_retry", {}),
    restart: () => transport.invoke("application_update_restart", {}),
  };
}

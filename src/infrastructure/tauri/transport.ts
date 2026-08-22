import { Channel, invoke as tauriInvoke } from "@tauri-apps/api/core";

import type {
  CancellableCommandArgs,
  CancellableTauriCommandName,
  CommandArgs,
  CommandResult,
  PlainTauriCommandName,
} from "@/infrastructure/tauri/command-contract";

export interface TypedTauriTransport {
  invoke<K extends PlainTauriCommandName>(command: K, args: CommandArgs<K>): Promise<CommandResult<K>>;
  invokeCancellable<K extends CancellableTauriCommandName>(
    command: K,
    args: CancellableCommandArgs<K>,
    signal: AbortSignal,
  ): Promise<CommandResult<K>>;
}

type RawTauriInvoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

/** WDIO 构建记录应用 adapter 发出的 command；生产构建完全不暴露该观测面。 */
function observeWdioInvoke(command: string) {
  if (import.meta.env.VITE_WDIO !== "true") return;
  const target = globalThis as typeof globalThis & { __NARRANT_E2E_TAURI_INVOKES__?: string[] };
  (target.__NARRANT_E2E_TAURI_INVOKES__ ??= []).push(command);
}

let nextOperationId = 0;

/** 创建跨 transport、页面重载和 WebView 都不会复用的操作身份。 */
function createOperationId(): string {
  const randomId = globalThis.crypto?.randomUUID?.();
  if (randomId) return `operation-${randomId}`;
  return `operation-${Date.now()}-${nextOperationId++}-${Math.random().toString(36).slice(2)}`;
}

/** 将 Tauri 的 stringly-typed invoke 收口为应用共享的 typed transport。 */
export function createTauriTransport(rawInvoke: RawTauriInvoke = tauriInvoke): TypedTauriTransport {
  return {
    invoke(command, args) {
      observeWdioInvoke(command);
      return rawInvoke(command, args) as Promise<never>;
    },

    async invokeCancellable(command, args, signal) {
      if (signal.aborted) throw new DOMException("Operation aborted", "AbortError");
      const operationId = createOperationId();
      observeWdioInvoke(command);
      const cancel = () => {
        void rawInvoke("cancel_operation", { operationId }).catch(() => undefined);
      };
      signal.addEventListener("abort", cancel, { once: true });
      try {
        return await rawInvoke(command, { ...args, operationId }) as never;
      } finally {
        signal.removeEventListener("abort", cancel);
      }
    },
  };
}

export const tauriTransport = createTauriTransport();

/** 在唯一允许直接依赖 Tauri core 的 typed transport 层创建 IPC progress channel。 */
export function createTauriChannel<T>(onmessage: (message: T) => void): Channel<T> {
  return new Channel<T>(onmessage);
}

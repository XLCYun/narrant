import { invoke as tauriInvoke } from "@tauri-apps/api/core";

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
      return rawInvoke(command, args) as Promise<never>;
    },

    async invokeCancellable(command, args, signal) {
      if (signal.aborted) throw new DOMException("Operation aborted", "AbortError");
      const operationId = createOperationId();
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

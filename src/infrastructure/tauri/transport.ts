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

/** 将 Tauri 的 stringly-typed invoke 收口为应用共享的 typed transport。 */
export function createTauriTransport(rawInvoke: RawTauriInvoke = tauriInvoke): TypedTauriTransport {
  let nextOperationId = 0;

  return {
    invoke(command, args) {
      return rawInvoke(command, args) as Promise<never>;
    },

    async invokeCancellable(command, args, signal) {
      if (signal.aborted) throw new DOMException("Operation aborted", "AbortError");
      const operationId = `git-operation-${nextOperationId++}`;
      const cancel = () => {
        void rawInvoke("cancel_git_operation", { operationId }).catch(() => undefined);
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

import type { LanguageSupportApplication } from "@/language-support/types";
import type { TypedTauriTransport } from "@/infrastructure/tauri/transport";

/** 将语言包管理接到本地命令，轮询不涉及网络请求。 */
export function createTauriLanguageSupportAdapter(transport: TypedTauriTransport): LanguageSupportApplication {
  return {
    snapshot: () => transport.invoke("language_support_snapshot", {}),
    retry: (id) => transport.invoke("language_support_retry", { id }),
    remove: (id, version) => transport.invoke("language_support_remove", { id, version }),
  };
}

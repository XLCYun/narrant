import { open as openDialog } from "@tauri-apps/plugin-dialog";

import type { ApplicationSettingsApplication, ThemePreference } from "@/settings/application-settings";
import type { TypedTauriTransport } from "@/infrastructure/tauri/transport";

type CodexExecutablePicker = () => Promise<string | string[] | null>;

/** 把 Settings 应用端口映射到原生文件选择器与集中 typed Tauri commands。 */
export function createTauriApplicationSettingsAdapter(
  transport: TypedTauriTransport,
  pickCodexExecutable: CodexExecutablePicker = () => openDialog({ multiple: false, directory: false }),
): ApplicationSettingsApplication {
  return {
    load: () => transport.invoke("load_application_settings", {}),
    updateTheme: (theme: ThemePreference) => transport.invoke("update_application_theme", { theme }),
    async chooseCodexExecutable() {
      const executable = await pickCodexExecutable();
      if (typeof executable !== "string" || executable.length === 0) return null;
      return transport.invoke("update_application_codex_executable", { executable });
    },
    clearCodexExecutable: () => transport.invoke("clear_application_codex_executable", {}),
  };
}

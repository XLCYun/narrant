import type { ApplicationSettingsApplication, ThemePreference } from "@/settings/application-settings";
import type { TypedTauriTransport } from "@/infrastructure/tauri/transport";

/** 把 Settings 应用端口映射到集中、可替换的 typed Tauri commands。 */
export function createTauriApplicationSettingsAdapter(transport: TypedTauriTransport): ApplicationSettingsApplication {
  return {
    load: () => transport.invoke("load_application_settings", {}),
    updateTheme: (theme: ThemePreference) => transport.invoke("update_application_theme", { theme }),
  };
}

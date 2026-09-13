import { open as openDialog } from "@tauri-apps/plugin-dialog";

import type { ApplicationLanguage, ApplicationSettingsApplication, BabysitProvider, BabysitProviderPreference, CustomProviderConnection, ThemePreference } from "@/settings/application-settings";
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
    updateLanguage: (language: ApplicationLanguage) => transport.invoke("update_application_language", { language }),
    updateRustDependencyScope: (scope) => transport.invoke("update_application_rust_dependency_scope", { scope }),
    listBabysitModels: (provider: BabysitProvider) => transport.invoke("list_babysit_models", { provider }),
    updateBabysitProvider: (provider: BabysitProvider) => transport.invoke("update_application_babysit_provider", { provider }),
    updateBabysitRepositoryExploration: (enabled: boolean) => transport.invoke("update_application_babysit_repository_exploration", { enabled }),
    updateBabysitAllNonEmptyLines: (enabled: boolean) => transport.invoke("update_application_babysit_all_non_empty_lines", { enabled }),
    async chooseBabysitExecutable(provider: BabysitProvider, preference: BabysitProviderPreference) {
      const executable = await pickCodexExecutable();
      if (typeof executable !== "string" || executable.length === 0) return null;
      return transport.invoke("update_application_babysit_preferences", { provider, preference: { ...preference, executable }, catalogRevision: null });
    },
    updateBabysitPreferences: (provider: BabysitProvider, preference: BabysitProviderPreference, catalogRevision: string | null = null) => transport.invoke("update_application_babysit_preferences", { provider, preference, catalogRevision }),
    loadCustomConnection: () => transport.invoke("load_application_custom_connection", {}),
    saveCustomConnection: (connection: CustomProviderConnection) => transport.invoke("save_application_custom_connection", { connection }),
  };
}

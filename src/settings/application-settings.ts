export type ThemePreference = "system" | "light" | "dark";

/** 前端消费的设备级 Application Settings 公共投影。 */
export type ApplicationSettings = {
  version: number;
  theme: ThemePreference;
  codexExecutable: string | null;
  babysitModel: unknown;
};

/** Settings UI 与持久化实现之间的窄应用端口。 */
export interface ApplicationSettingsApplication {
  load(): Promise<ApplicationSettings>;
  updateTheme(theme: ThemePreference): Promise<ApplicationSettings>;
}

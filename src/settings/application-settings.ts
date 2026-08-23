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
  /** 打开原生文件选择器，并在后端验证通过后保存选中的 Codex。 */
  chooseCodexExecutable(): Promise<ApplicationSettings | null>;
  /** 清空显式 Codex Executable Preference，恢复自动发现。 */
  clearCodexExecutable(): Promise<ApplicationSettings>;
}

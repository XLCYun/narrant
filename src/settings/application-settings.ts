export type ThemePreference = "system" | "light" | "dark";

/** App Server 动态公布的思考强度标识；前端不维护封闭枚举。 */
export type ReasoningEffort = string;

/** 未来分析使用的自动或显式 Babysit Model Preference。 */
export type BabysitModelPreference =
  | { kind: "automatic" }
  | { kind: "explicit"; modelId: string; reasoningEffort: ReasoningEffort | null };

/** App Server 返回的一个可见模型及其当前思考强度能力。 */
export type BabysitModelCatalogEntry = {
  id: string;
  name: string;
  isDefault: boolean;
  supportedReasoningEfforts: ReasoningEffort[];
  defaultReasoningEffort: ReasoningEffort;
};

/** 一次完整、仅驻留内存的动态模型目录。 */
export type BabysitModelCatalog = { revision: string; models: BabysitModelCatalogEntry[] };

/** 前端消费的设备级 Application Settings 公共投影。 */
export type ApplicationSettings = {
  version: number;
  theme: ThemePreference;
  codexExecutable: string | null;
  babysitModel: BabysitModelPreference;
};

/** Settings UI 与持久化实现之间的窄应用端口。 */
export interface ApplicationSettingsApplication {
  /** 读取当前设备级 Application Settings 快照。 */
  load(): Promise<ApplicationSettings>;
  /** 原子更新主题偏好。 */
  updateTheme(theme: ThemePreference): Promise<ApplicationSettings>;
  /** 打开原生文件选择器，并在后端验证通过后保存选中的 Codex。 */
  chooseCodexExecutable(): Promise<ApplicationSettings | null>;
  /** 清空显式 Codex Executable Preference，恢复自动发现。 */
  clearCodexExecutable(): Promise<ApplicationSettings>;
  /** 仅在用户进入修改态时获取当前完整模型目录。 */
  listBabysitModels(): Promise<BabysitModelCatalog>;
  /** 由后端再次校验目录组合后原子保存模型偏好。 */
  updateBabysitModel(preference: BabysitModelPreference, catalogRevision: string): Promise<ApplicationSettings>;
}

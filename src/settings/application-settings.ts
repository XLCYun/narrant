export type ThemePreference = "system" | "light" | "dark";

/** Application Settings 支持的稳定界面语言；后端和文档语言都使用这些 BCP-47 值。 */
export type ApplicationLanguage = "zh-CN" | "en-US";

/** App Server 动态公布的思考强度标识；前端不维护封闭枚举。 */
export type ReasoningEffort = string;

/** Babysit 支持的设备级 Agent Provider。 */
export type BabysitProvider = "codex" | "claude-code" | "custom";

/** Custom 内置 provider 支持的显式模型服务协议。 */
export type CustomProviderProtocol = "openai_chat_completions" | "anthropic_messages";

/** Custom 连接使用的 Provider 默认值或显式思考强度。 */
export type CustomProviderEffort = "provider_default" | "low" | "medium" | "high";

/** Custom 设置中可持久化的非敏感连接字段。 */
export type CustomProviderSettings = {
  protocol: CustomProviderProtocol;
  baseUrl: string;
  model: string;
  effort: CustomProviderEffort;
};

/** Custom 编辑器一次读写的完整连接快照；API key 只在内存中传递。 */
export type CustomProviderConnection = CustomProviderSettings & {
  apiKey: string;
};

/** 一个 provider 独立保存的完整偏好组合。 */
export type BabysitProviderPreference = {
  executable: string | null;
  model: string | null;
  reasoningEffort: ReasoningEffort | null;
};

/** App Server 返回的一个可见模型及其当前思考强度能力。 */
export type BabysitModelCatalogEntry = {
  id: string;
  name: string;
  isDefault: boolean;
  supportedReasoningEfforts: ReasoningEffort[];
  defaultReasoningEffort: ReasoningEffort;
};

/** 一次完整、仅驻留内存的动态模型目录。 */
export type BabysitModelCatalog = { provider: BabysitProvider; revision: string; models: BabysitModelCatalogEntry[] };

/** 所选 provider 与两套互不覆盖的 Provider Preference。 */
export type BabysitSettings = {
  provider: BabysitProvider;
  codex: BabysitProviderPreference;
  claudeCode: BabysitProviderPreference;
  custom: CustomProviderSettings;
};

/** 前端消费的设备级 Application Settings 公共投影。 */
export type ApplicationSettings = {
  version: number;
  theme: ThemePreference;
  language: ApplicationLanguage;
  babysit: BabysitSettings;
};

/** Settings UI 与持久化实现之间的窄应用端口。 */
export interface ApplicationSettingsApplication {
  /** 读取当前设备级 Application Settings 快照。 */
  load(): Promise<ApplicationSettings>;
  /** 原子更新主题偏好。 */
  updateTheme(theme: ThemePreference): Promise<ApplicationSettings>;
  /** 原子更新应用界面语言偏好。 */
  updateLanguage(language: ApplicationLanguage): Promise<ApplicationSettings>;
  /** 仅在用户进入修改态时获取当前完整模型目录。 */
  listBabysitModels(provider: BabysitProvider): Promise<BabysitModelCatalog>;
  /** 立即原子保存未来分析使用的 provider。 */
  updateBabysitProvider(provider: BabysitProvider): Promise<ApplicationSettings>;
  /** 通过原生 picker 更新指定 provider 的 executable，并保留其模型组合。 */
  chooseBabysitExecutable(provider: BabysitProvider, preference: BabysitProviderPreference): Promise<ApplicationSettings | null>;
  /** 原子保存指定 provider 的完整 executable/model/effort 组合。 */
  updateBabysitPreferences(provider: BabysitProvider, preference: BabysitProviderPreference, catalogRevision?: string | null): Promise<ApplicationSettings>;
  /** 读取 Custom 的完整连接；API key 只供设置编辑器短暂使用。 */
  loadCustomConnection(): Promise<CustomProviderConnection>;
  /** 原子保存 Custom 的完整连接并返回新的非敏感设置快照。 */
  saveCustomConnection(connection: CustomProviderConnection): Promise<ApplicationSettings>;
}

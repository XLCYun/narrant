import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

import { i18n, type SupportedLocale } from "@/lib/i18n";
import type { ApplicationLanguage, ApplicationSettings, ApplicationSettingsApplication, ThemePreference } from "@/settings/application-settings";

const SYSTEM_DARK_THEME_QUERY = "(prefers-color-scheme: dark)";

/** 将一个主题偏好解析并同步到根节点，作为 shell、Monaco 与品牌图像的共同信号。 */
function applyTheme(preference: ThemePreference, systemDark: boolean) {
  const dark = preference === "dark" || (preference === "system" && systemDark);
  document.documentElement.classList.toggle("dark", dark);
  return dark;
}

/** 将持久化语言值映射到现有离线 i18n 资源的 key。 */
function localeForLanguage(language: ApplicationLanguage): SupportedLocale {
  return language === "en-US" ? "en" : "zh-CN";
}

/** 把一个 Application Language 同步到 i18n 实例与根文档可访问语言标记。 */
function applyLanguage(language: ApplicationLanguage) {
  document.documentElement.lang = language;
  void i18n.changeLanguage(localeForLanguage(language));
}

/** 兼容旧 settings v1 的暂时缺字段快照；正式快照由 Rust 负责补出语言。 */
function languageFromSettings(settings: ApplicationSettings, fallback: ApplicationLanguage): ApplicationLanguage {
  return settings.language ?? fallback;
}

/** 用一个设置快照加载并同步主题和语言，向设置页提供完整的应用设置 binding。 */
export function useApplicationSettings(application: ApplicationSettingsApplication, enabled = true) {
  const media = useRef<MediaQueryList>();
  const [preference, setPreference] = useState<ThemePreference>("system");
  const [language, setLanguage] = useState<ApplicationLanguage>(() => (
    i18n.resolvedLanguage?.toLowerCase().startsWith("en") ? "en-US" : "zh-CN"
  ));
  const languageRef = useRef(language);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const [languageSaving, setLanguageSaving] = useState(false);
  const languageSavingRef = useRef(false);
  const [editorTheme, setEditorTheme] = useState<"light" | "vs-dark">("light");

  const sync = useCallback((next: ThemePreference, systemDark = media.current?.matches ?? false) => {
    setEditorTheme(applyTheme(next, systemDark) ? "vs-dark" : "light");
  }, []);

  useLayoutEffect(() => {
    const query = window.matchMedia(SYSTEM_DARK_THEME_QUERY);
    media.current = query;
    const onChange = (event: MediaQueryListEvent) => {
      if (preference === "system") sync("system", event.matches);
    };
    sync(preference, query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, [preference, sync]);

  useEffect(() => {
    if (!enabled) {
      setLoaded(true);
      return;
    }
    let current = true;
    void Promise.resolve(application.load()).then((settings) => {
      if (!current) return;
      setPreference(settings.theme);
      sync(settings.theme);
      const nextLanguage = languageFromSettings(settings, languageRef.current);
      languageRef.current = nextLanguage;
      setLanguage(nextLanguage);
      applyLanguage(nextLanguage);
      setLoaded(true);
    }).catch((reason) => {
      if (!current) return;
      setError(`未能读取主题设置：${String(reason)}`);
      setLoaded(true);
    });
    return () => { current = false; };
  }, [application, enabled, sync]);

  /** 先提交视觉选择，再保存；失败时恢复原偏好与相同的有效主题。 */
  const choose = useCallback(async (next: ThemePreference) => {
    if (savingRef.current || next === preference) return;
    savingRef.current = true;
    setSaving(true);
    const previous = preference;
    setError(undefined);
    setPreference(next);
    sync(next);
    try {
      const saved = await application.updateTheme(next);
      setPreference(saved.theme ?? next);
      sync(saved.theme ?? next);
    } catch (reason) {
      setPreference(previous);
      sync(previous);
      setError(`未能保存主题设置：${String(reason)}。请重试。`);
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [application, preference, sync]);

  /** 只有语言持久化成功后才切换界面；失败时全程保持旧语言并展示错误。 */
  const chooseLanguage = useCallback(async (next: ApplicationLanguage) => {
    if (languageSavingRef.current || next === language) return;
    languageSavingRef.current = true;
    setLanguageSaving(true);
    const previous = language;
    setError(undefined);
    try {
      const saved = await application.updateLanguage(next);
      const savedLanguage = languageFromSettings(saved, next);
      languageRef.current = savedLanguage;
      setLanguage(savedLanguage);
      applyLanguage(savedLanguage);
    } catch (reason) {
      languageRef.current = previous;
      setLanguage(previous);
      applyLanguage(previous);
      setError(`未能保存语言设置：${String(reason)}。请重试。`);
    } finally {
      languageSavingRef.current = false;
      setLanguageSaving(false);
    }
  }, [application, language]);

  return { preference, language, loaded, saving, languageSaving, error, editorTheme, choose, chooseLanguage };
}

export type ApplicationSettingsBinding = ReturnType<typeof useApplicationSettings>;

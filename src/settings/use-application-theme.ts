import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

import type { ApplicationSettingsApplication, ThemePreference } from "@/settings/application-settings";

const SYSTEM_DARK_THEME_QUERY = "(prefers-color-scheme: dark)";

/** 将一个主题偏好解析并同步到根节点，作为 shell、Monaco 与品牌图像的共同信号。 */
function applyTheme(preference: ThemePreference, systemDark: boolean) {
  const dark = preference === "dark" || (preference === "system" && systemDark);
  document.documentElement.classList.toggle("dark", dark);
  return dark;
}

/** 加载、即时应用并持久化主题；保存失败时可见地恢复上一偏好。 */
export function useApplicationTheme(application: ApplicationSettingsApplication, enabled = true) {
  const media = useRef<MediaQueryList>();
  const [preference, setPreference] = useState<ThemePreference>("system");
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
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
      setPreference(saved.theme);
      sync(saved.theme);
    } catch (reason) {
      setPreference(previous);
      sync(previous);
      setError(`未能保存主题设置：${String(reason)}。请重试。`);
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [application, preference, sync]);

  return { preference, loaded, saving, error, editorTheme, choose };
}

export type ApplicationThemeBinding = ReturnType<typeof useApplicationTheme>;

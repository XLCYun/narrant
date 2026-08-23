import { useCallback, useEffect, useRef, useState } from "react";

import type { ApplicationSettingsApplication } from "@/settings/application-settings";

/** 加载并更新 Codex Executable Preference；失败时始终保留上一份持久化投影。 */
export function useCodexExecutablePreference(application: ApplicationSettingsApplication, enabled = true) {
  const [executable, setExecutable] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const savingRef = useRef(false);

  useEffect(() => {
    if (!enabled) {
      setLoaded(true);
      return;
    }
    let current = true;
    void application.load().then((settings) => {
      if (!current) return;
      setExecutable(settings.codexExecutable);
      setLoaded(true);
    }).catch((reason) => {
      if (!current) return;
      setError(`未能读取 Codex 设置：${String(reason)}。请重试。`);
      setLoaded(true);
    });
    return () => { current = false; };
  }, [application, enabled]);

  /** 串行打开 picker 与保存，取消选择时不改变当前投影。 */
  const choose = useCallback(async () => {
    if (savingRef.current) return;
    savingRef.current = true;
    setSaving(true);
    setError(undefined);
    try {
      const saved = await application.chooseCodexExecutable();
      if (saved) setExecutable(saved.codexExecutable);
    } catch (reason) {
      setError(`未能保存 Codex 可执行文件：${String(reason)}。原设置已保留，请重新选择。`);
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [application]);

  /** 清空成功后才切回自动发现；失败仍展示原路径。 */
  const clear = useCallback(async () => {
    if (savingRef.current || !executable) return;
    savingRef.current = true;
    setSaving(true);
    setError(undefined);
    try {
      const saved = await application.clearCodexExecutable();
      setExecutable(saved.codexExecutable);
    } catch (reason) {
      setError(`未能恢复自动发现：${String(reason)}。原设置已保留，请重试。`);
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [application, executable]);

  return { executable, loaded, saving, error, choose, clear };
}

export type CodexExecutablePreferenceBinding = ReturnType<typeof useCodexExecutablePreference>;

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import { ApplicationSettingsDestination } from "@/components/settings-page";
import { createTauriApplicationSettingsAdapter } from "@/infrastructure/tauri-application-settings-adapter";
import { i18n } from "@/lib/i18n";
import type { ApplicationSettings, ApplicationSettingsApplication } from "@/settings/application-settings";

afterEach(async () => {
  cleanup();
  document.documentElement.lang = "";
  vi.unstubAllGlobals();
  await i18n.changeLanguage("zh-CN");
});

function application(language: "zh-CN" | "en-US" = "zh-CN"): ApplicationSettingsApplication {
  const babysit = {
    provider: "codex" as const,
    codex: { executable: null, model: null, reasoningEffort: null },
    claudeCode: { executable: null, model: null, reasoningEffort: null },
    custom: { protocol: "openai_chat_completions" as const, baseUrl: "", model: "", effort: "medium" as const },
  };
  return {
    load: vi.fn(async () => ({ version: 1, theme: "system" as const, language, babysit })),
    updateTheme: vi.fn(async (theme) => ({ version: 1, theme, language, babysit })),
    updateLanguage: vi.fn(async (nextLanguage) => ({ version: 1, theme: "system" as const, language: nextLanguage, babysit })),
    listBabysitModels: vi.fn(async (provider) => ({ provider, revision: "catalog", models: [] })),
    updateBabysitProvider: vi.fn(async (provider) => ({ version: 1, theme: "system" as const, language, babysit: { ...babysit, provider } })),
    chooseBabysitExecutable: vi.fn(async () => ({ version: 1, theme: "system" as const, language, babysit })),
    updateBabysitPreferences: vi.fn(async () => ({ version: 1, theme: "system" as const, language, babysit })),
    loadCustomConnection: vi.fn(async () => ({ ...babysit.custom, apiKey: "" })),
    saveCustomConnection: vi.fn(async (connection) => ({ version: 1, theme: "system" as const, language, babysit: { ...babysit, custom: { protocol: connection.protocol, baseUrl: connection.baseUrl, model: connection.model, effort: connection.effort } } })),
  };
}

describe("Application Language UI", () => {
  it("typed adapter maps the public language update command", async () => {
    const invoke = vi.fn(async () => undefined);
    const adapter = createTauriApplicationSettingsAdapter({ invoke } as never);

    await adapter.updateLanguage("en-US");

    expect(invoke).toHaveBeenCalledWith("update_application_language", { language: "en-US" });
  });

  it("loads one settings language and synchronizes i18n, html lang, and selected state", async () => {
    const settings = application("en-US");
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);

    const english = await screen.findByRole("radio", { name: "English" });
    expect(english).toHaveAttribute("aria-checked", "true");
    expect(i18n.resolvedLanguage).toBe("en");
    expect(document.documentElement.lang).toBe("en-US");
    expect(screen.getByRole("radio", { name: "简体中文" })).toHaveAttribute("aria-checked", "false");
    expect(settings.load).toHaveBeenCalledTimes(1);
  });

  it("persists the Application Language before applying Simplified Chinese", async () => {
    const settings = application("en-US");
    let resolveSave!: (settings: ApplicationSettings) => void;
    const pendingSave = new Promise<ApplicationSettings>((resolve) => { resolveSave = resolve; });
    vi.mocked(settings.updateLanguage).mockReturnValueOnce(pendingSave);
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    await screen.findByRole("radio", { name: "English" });

    fireEvent.click(screen.getByRole("radio", { name: "简体中文" }));

    await waitFor(() => expect(settings.updateLanguage).toHaveBeenCalledWith("zh-CN"));
    expect(i18n.resolvedLanguage).toBe("en");
    expect(document.documentElement.lang).toBe("en-US");
    expect(screen.getByRole("radio", { name: "English" })).toHaveAttribute("aria-checked", "true");

    resolveSave({
      version: 1,
      theme: "system",
      language: "zh-CN",
      babysit: {
        provider: "codex",
        codex: { executable: null, model: null, reasoningEffort: null },
        claudeCode: { executable: null, model: null, reasoningEffort: null },
        custom: { protocol: "openai_chat_completions", baseUrl: "", model: "", effort: "medium" },
      },
    });

    await waitFor(() => expect(i18n.resolvedLanguage).toBe("zh-CN"));
    expect(i18n.resolvedLanguage).toBe("zh-CN");
    expect(document.documentElement.lang).toBe("zh-CN");
    expect(screen.getByRole("radio", { name: "简体中文" })).toHaveAttribute("aria-checked", "true");
  });

  it("blocks theme submission while language persistence is in flight", async () => {
    const settings = application("en-US");
    let resolveSave!: (settings: ApplicationSettings) => void;
    vi.mocked(settings.updateLanguage).mockReturnValueOnce(new Promise((resolve) => { resolveSave = resolve; }));
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    await screen.findByRole("radio", { name: "English" });

    fireEvent.click(screen.getByRole("radio", { name: "简体中文" }));
    await waitFor(() => expect(settings.updateLanguage).toHaveBeenCalledWith("zh-CN"));

    const dark = screen.getByRole("radio", { name: /^深色/ });
    expect(dark).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("radio", { name: "English" })).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(dark);
    expect(settings.updateTheme).not.toHaveBeenCalled();

    resolveSave({
      version: 1,
      theme: "system",
      language: "zh-CN",
      babysit: {
        provider: "codex",
        codex: { executable: null, model: null, reasoningEffort: null },
        claudeCode: { executable: null, model: null, reasoningEffort: null },
        custom: { protocol: "openai_chat_completions", baseUrl: "", model: "", effort: "medium" },
      },
    });
    await waitFor(() => expect(screen.getByRole("radio", { name: "简体中文" })).toHaveAttribute("aria-checked", "true"));
  });

  it("blocks language submission while theme persistence is in flight", async () => {
    const settings = application("en-US");
    let resolveSave!: (settings: ApplicationSettings) => void;
    vi.mocked(settings.updateTheme).mockReturnValueOnce(new Promise((resolve) => { resolveSave = resolve; }));
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    await screen.findByRole("radio", { name: "English" });

    fireEvent.click(screen.getByRole("radio", { name: /^深色/ }));
    await waitFor(() => expect(settings.updateTheme).toHaveBeenCalledWith("dark"));

    const chinese = screen.getByRole("radio", { name: "简体中文" });
    expect(chinese).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("radio", { name: /^深色/ })).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(chinese);
    expect(settings.updateLanguage).not.toHaveBeenCalled();

    resolveSave({
      version: 1,
      theme: "dark",
      language: "en-US",
      babysit: {
        provider: "codex",
        codex: { executable: null, model: null, reasoningEffort: null },
        claudeCode: { executable: null, model: null, reasoningEffort: null },
        custom: { protocol: "openai_chat_completions", baseUrl: "", model: "", effort: "medium" },
      },
    });
    await waitFor(() => expect(screen.getByRole("radio", { name: /^深色/ })).toHaveAttribute("aria-checked", "true"));
  });

  it("language persistence failure restores the previous i18n and document language", async () => {
    const settings = application("en-US");
    vi.mocked(settings.updateLanguage).mockRejectedValueOnce(new Error("settings.json is read-only"));
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    await screen.findByRole("radio", { name: "English" });

    fireEvent.click(screen.getByRole("radio", { name: "简体中文" }));
    expect(i18n.resolvedLanguage).toBe("en");
    expect(document.documentElement.lang).toBe("en-US");
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("settings.json is read-only"));

    expect(i18n.resolvedLanguage).toBe("en");
    expect(document.documentElement.lang).toBe("en-US");
    expect(screen.getByRole("radio", { name: "English" })).toHaveAttribute("aria-checked", "true");
  });

  it("does not read or write the retired locale localStorage key", async () => {
    const getItem = vi.spyOn(window.localStorage, "getItem");
    const setItem = vi.spyOn(window.localStorage, "setItem");
    const settings = application("en-US");
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    await screen.findByRole("radio", { name: "English" });
    fireEvent.click(screen.getByRole("radio", { name: "简体中文" }));
    await waitFor(() => expect(settings.updateLanguage).toHaveBeenCalled());

    expect(getItem).not.toHaveBeenCalledWith("narrant.locale");
    expect(setItem).not.toHaveBeenCalledWith("narrant.locale", expect.anything());
  });
});

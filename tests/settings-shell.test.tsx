import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import { ApplicationSettingsDestination } from "@/components/settings-page";
import type { ApplicationSettingsApplication } from "@/settings/application-settings";
import { createTauriApplicationSettingsAdapter } from "@/infrastructure/tauri-application-settings-adapter";

afterEach(() => {
  cleanup();
  document.documentElement.classList.remove("dark");
  vi.unstubAllGlobals();
});

function mockSystemTheme(initialDark: boolean) {
  let dark = initialDark;
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  vi.stubGlobal("matchMedia", vi.fn(() => ({
    get matches() { return dark; },
    media: "(prefers-color-scheme: dark)",
    onchange: null,
    addEventListener: (_type: string, listener: (event: MediaQueryListEvent) => void) => listeners.add(listener),
    removeEventListener: (_type: string, listener: (event: MediaQueryListEvent) => void) => listeners.delete(listener),
    dispatchEvent: vi.fn(), addListener: vi.fn(), removeListener: vi.fn(),
  })));
  return (nextDark: boolean) => {
    dark = nextDark;
    listeners.forEach((listener) => listener({ matches: dark } as MediaQueryListEvent));
  };
}

function application(theme: "system" | "light" | "dark" = "system"): ApplicationSettingsApplication {
  return {
    load: vi.fn(async () => ({ version: 1, theme, codexExecutable: null, babysitModel: { kind: "automatic" } })),
    updateTheme: vi.fn(async (nextTheme) => ({ version: 1, theme: nextTheme, codexExecutable: null, babysitModel: { kind: "automatic" } })),
  };
}

describe("Settings application-shell UI", () => {
  it("通过约定的 Tauri commands 读取并更新完整 Application Settings", async () => {
    const invoke = vi.fn(async (_command: string, args: Record<string, unknown>) => ({
      version: 1, theme: args.theme ?? "system", codexExecutable: null, babysitModel: { kind: "automatic" },
    }));
    const adapter = createTauriApplicationSettingsAdapter({ invoke } as never);

    await adapter.load();
    await adapter.updateTheme("dark");

    expect(invoke).toHaveBeenNthCalledWith(1, "load_application_settings", {});
    expect(invoke).toHaveBeenNthCalledWith(2, "update_application_theme", { theme: "dark" });
  });

  it("展示可扩展二级导航，并为后续分区提供稳定占位", async () => {
    mockSystemTheme(false);
    render(<ApplicationSettingsDestination application={application()} onBack={() => undefined} />);

    expect(await screen.findByRole("heading", { name: "应用设置" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "返回" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "GitHub" }));
    expect(screen.getByRole("heading", { name: "GitHub" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "宝宝模式" }));
    expect(screen.getByText("宝宝模式设置将在后续版本中提供。" )).toBeInTheDocument();
  });

  it("主题选择立即生效，持久化失败时恢复旧主题并显示可操作错误", async () => {
    mockSystemTheme(false);
    let rejectSave!: (reason: Error) => void;
    const settings = application("light");
    vi.mocked(settings.updateTheme).mockImplementation(() => new Promise((_resolve, reject) => { rejectSave = reject; }));
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    await screen.findByRole("radio", { name: /^浅色/ });

    fireEvent.click(screen.getByRole("radio", { name: /^深色/ }));
    expect(document.documentElement).toHaveClass("dark");
    await act(async () => rejectSave(new Error("磁盘已满")));

    await waitFor(() => expect(document.documentElement).not.toHaveClass("dark"));
    expect(screen.getByRole("alert")).toHaveTextContent("未能保存主题设置");
    expect(screen.getByRole("radio", { name: /^浅色/ })).toHaveAttribute("aria-checked", "true");
  });

  it("跟随系统时响应系统主题变化，显式主题不再跟随", async () => {
    const changeSystemTheme = mockSystemTheme(false);
    const settings = application("system");
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    await screen.findByRole("radio", { name: /^跟随系统/ });

    act(() => changeSystemTheme(true));
    expect(document.documentElement).toHaveClass("dark");
    fireEvent.click(screen.getByRole("radio", { name: /^浅色/ }));
    act(() => changeSystemTheme(true));
    expect(document.documentElement).not.toHaveClass("dark");
  });
});

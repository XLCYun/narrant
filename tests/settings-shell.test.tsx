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
    chooseCodexExecutable: vi.fn(async () => ({ version: 1, theme, codexExecutable: "C:\\Program Files\\Codex\\codex.exe", babysitModel: { kind: "automatic" } })),
    clearCodexExecutable: vi.fn(async () => ({ version: 1, theme, codexExecutable: null, babysitModel: { kind: "automatic" } })),
  };
}

describe("Settings application-shell UI", () => {
  it("通过约定的 Tauri commands 读取并更新完整 Application Settings", async () => {
    const invoke = vi.fn(async (_command: string, args: Record<string, unknown>) => ({
      version: 1, theme: args.theme ?? "system", codexExecutable: null, babysitModel: { kind: "automatic" },
    }));
    const adapter = createTauriApplicationSettingsAdapter({ invoke } as never, vi.fn(async () => "C:\\Tools\\codex.exe"));

    await adapter.load();
    await adapter.updateTheme("dark");
    await adapter.chooseCodexExecutable();
    await adapter.clearCodexExecutable();

    expect(invoke).toHaveBeenNthCalledWith(1, "load_application_settings", {});
    expect(invoke).toHaveBeenNthCalledWith(2, "update_application_theme", { theme: "dark" });
    expect(invoke).toHaveBeenNthCalledWith(3, "update_application_codex_executable", { executable: "C:\\Tools\\codex.exe" });
    expect(invoke).toHaveBeenNthCalledWith(4, "clear_application_codex_executable", {});
  });

  it("展示可扩展二级导航，并为后续分区提供稳定占位", async () => {
    mockSystemTheme(false);
    render(<ApplicationSettingsDestination application={application()} onBack={() => undefined} />);

    expect(await screen.findByRole("heading", { name: "应用设置" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "返回" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "GitHub" }));
    expect(screen.getByRole("heading", { name: "GitHub" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "宝宝模式" }));
    expect(screen.getByRole("heading", { name: "宝宝模式" })).toBeInTheDocument();
  });

  it("通过原生文件选择器保存 Codex Executable Preference", async () => {
    mockSystemTheme(false);
    const settings = application();
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    fireEvent.click(await screen.findByRole("tab", { name: "宝宝模式" }));

    expect(screen.getByText("自动发现")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "选择 Codex 可执行文件" }));

    expect(await screen.findByText("C:\\Program Files\\Codex\\codex.exe")).toBeInTheDocument();
    expect(settings.chooseCodexExecutable).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });

  it("取消原生文件选择时不调用保存 command", async () => {
    const invoke = vi.fn();
    const adapter = createTauriApplicationSettingsAdapter({ invoke } as never, vi.fn(async () => null));

    await expect(adapter.chooseCodexExecutable()).resolves.toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("验证失败时保留旧路径并显示可操作错误", async () => {
    mockSystemTheme(false);
    const oldPath = "C:\\Tools\\codex.exe";
    const settings = application();
    vi.mocked(settings.load).mockResolvedValue({ version: 1, theme: "system", codexExecutable: oldPath, babysitModel: { kind: "automatic" } });
    vi.mocked(settings.chooseCodexExecutable).mockRejectedValue(new Error("版本探测失败"));
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    fireEvent.click(await screen.findByRole("tab", { name: "宝宝模式" }));
    expect(await screen.findByText(oldPath)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "选择 Codex 可执行文件" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("原设置已保留");
    expect(screen.getByText(oldPath)).toBeInTheDocument();
  });

  it("清空显式路径后恢复自动发现", async () => {
    mockSystemTheme(false);
    const settings = application();
    vi.mocked(settings.load).mockResolvedValue({ version: 1, theme: "system", codexExecutable: "C:\\Tools\\codex.exe", babysitModel: { kind: "automatic" } });
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    fireEvent.click(await screen.findByRole("tab", { name: "宝宝模式" }));
    fireEvent.click(await screen.findByRole("button", { name: "恢复自动发现" }));

    expect(await screen.findByText("自动发现")).toBeInTheDocument();
    expect(settings.clearCodexExecutable).toHaveBeenCalledTimes(1);
  });

  it("验证保存期间禁用重复选择", async () => {
    mockSystemTheme(false);
    let finish!: () => void;
    const settings = application();
    vi.mocked(settings.chooseCodexExecutable).mockImplementation(() => new Promise((resolve) => {
      finish = () => resolve({ version: 1, theme: "system", codexExecutable: "C:\\Tools\\codex.exe", babysitModel: { kind: "automatic" } });
    }));
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    fireEvent.click(await screen.findByRole("tab", { name: "宝宝模式" }));
    fireEvent.click(screen.getByRole("button", { name: "选择 Codex 可执行文件" }));

    expect(screen.getByRole("button", { name: "正在验证…" })).toBeDisabled();
    await act(async () => finish());
    expect(screen.getByText("C:\\Tools\\codex.exe")).toBeInTheDocument();
  });

  it("清空失败时保留旧路径", async () => {
    mockSystemTheme(false);
    const oldPath = "C:\\Tools\\codex.exe";
    const settings = application();
    vi.mocked(settings.load).mockResolvedValue({ version: 1, theme: "system", codexExecutable: oldPath, babysitModel: { kind: "automatic" } });
    vi.mocked(settings.clearCodexExecutable).mockRejectedValue(new Error("写入失败"));
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    fireEvent.click(await screen.findByRole("tab", { name: "宝宝模式" }));
    fireEvent.click(await screen.findByRole("button", { name: "恢复自动发现" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("原设置已保留");
    expect(screen.getByText(oldPath)).toBeInTheDocument();
  });

  it("长路径允许换行且操作区保持可用", async () => {
    mockSystemTheme(false);
    const longPath = `C:\\${"very-long-directory\\".repeat(12)}codex.exe`;
    const settings = application();
    vi.mocked(settings.load).mockResolvedValue({ version: 1, theme: "system", codexExecutable: longPath, babysitModel: { kind: "automatic" } });
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    fireEvent.click(await screen.findByRole("tab", { name: "宝宝模式" }));

    expect(await screen.findByTestId("codex-executable-value")).toHaveClass("break-all");
    expect(screen.getByRole("button", { name: "选择 Codex 可执行文件" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "恢复自动发现" })).toBeEnabled();
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

  it("主题保存期间禁用其他选择，避免乱序结果覆盖用户决定", async () => {
    mockSystemTheme(false);
    let finishSave!: () => void;
    const settings = application("light");
    vi.mocked(settings.updateTheme).mockImplementation((theme) => new Promise((resolve) => {
      finishSave = () => resolve({ version: 1, theme, codexExecutable: null, babysitModel: { kind: "automatic" } });
    }));
    render(<ApplicationSettingsDestination application={settings} onBack={() => undefined} />);
    await screen.findByRole("radio", { name: /^浅色/ });

    fireEvent.click(screen.getByRole("radio", { name: /^深色/ }));
    fireEvent.click(screen.getByRole("radio", { name: /^跟随系统/ }));

    expect(settings.updateTheme).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("radio", { name: /^跟随系统/ })).toBeDisabled();
    await act(async () => finishSave());
    expect(screen.getByRole("radio", { name: /^深色/ })).toHaveAttribute("aria-checked", "true");
  });
});

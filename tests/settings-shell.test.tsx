import { afterEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import { ApplicationSettingsDestination } from "@/components/settings-page";
import type { ApplicationSettingsApplication } from "@/settings/application-settings";
import { createTauriApplicationSettingsAdapter } from "@/infrastructure/tauri-application-settings-adapter";
import type { GitHubIdentityLifecycleBinding } from "@/github/use-github-identity-lifecycle";

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
  const babysit = {
    provider: "codex" as const,
    codex: { executable: null, model: null, reasoningEffort: null },
    claudeCode: { executable: null, model: null, reasoningEffort: null },
  };
  return {
    load: vi.fn(async () => ({ version: 1, theme, babysit })),
    updateTheme: vi.fn(async (nextTheme) => ({ version: 1, theme: nextTheme, babysit })),
    listBabysitModels: vi.fn(async (provider) => ({ provider, revision: "catalog-1", models: [
      { id: "gpt-5.4", name: "GPT-5.4", isDefault: true, supportedReasoningEfforts: ["low", "high"], defaultReasoningEffort: "high" },
      { id: "gpt-5.4-mini", name: "GPT-5.4 Mini", isDefault: false, supportedReasoningEfforts: ["low", "medium"], defaultReasoningEffort: "medium" },
    ] })),
    updateBabysitProvider: vi.fn(async (provider) => ({ version: 1, theme, babysit: { ...babysit, provider } })),
    chooseBabysitExecutable: vi.fn(async () => ({ version: 1, theme, babysit })),
    updateBabysitPreferences: vi.fn(async () => ({ version: 1, theme, babysit })),
  };
}

/** 通过公开 combobox 语义选择一个动态设置项。 */
async function chooseSelect(label: string, option: string | RegExp) {
  fireEvent.click(screen.getByRole("combobox", { name: label }));
  const item = await screen.findByRole("option", { name: option });
  fireEvent.pointerDown(item);
  fireEvent.pointerUp(item);
  fireEvent.click(item);
}

/** 构造设置页消费的 GitHub Identity lifecycle 公共绑定。 */
function githubIdentity(): GitHubIdentityLifecycleBinding {
  return {
    status: { configured: true, signedIn: true, appInstallUrl: null, identity: { id: 42, login: "octocat", name: "Monalisa", avatarPath: "/cached/octocat.png" } },
    transition: { kind: "stable" },
    beginAuthorization: vi.fn(async () => ({ kind: "failed" })),
    completeAuthorization: vi.fn(async () => ({ kind: "failed" })),
    signOut: vi.fn(async () => ({ kind: "signed-out" })),
    retry: vi.fn(async () => undefined),
  };
}

describe("Settings application-shell UI", () => {
  it("typed adapter 原样提交 provider 与完整偏好组合", async () => {
    const invoke = vi.fn(async () => undefined);
    const adapter = createTauriApplicationSettingsAdapter({ invoke } as never, vi.fn(async () => "C:\\Tools\\claude.exe"));

    await adapter.updateBabysitProvider("claude-code");
    await adapter.chooseBabysitExecutable("claude-code", { executable: null, model: "sonnet", reasoningEffort: "high" });
    await adapter.updateBabysitPreferences("claude-code", { executable: "C:\\Tools\\claude.exe", model: "opus", reasoningEffort: "max" }, "claude-catalog-1");

    expect(invoke).toHaveBeenNthCalledWith(1, "update_application_babysit_provider", { provider: "claude-code" });
    expect(invoke).toHaveBeenNthCalledWith(2, "update_application_babysit_preferences", { provider: "claude-code", preference: { executable: "C:\\Tools\\claude.exe", model: "sonnet", reasoningEffort: "high" }, catalogRevision: null });
    expect(invoke).toHaveBeenNthCalledWith(3, "update_application_babysit_preferences", { provider: "claude-code", preference: { executable: "C:\\Tools\\claude.exe", model: "opus", reasoningEffort: "max" }, catalogRevision: "claude-catalog-1" });
  });

  it("只呈现当前 provider，并即时保存完整组合且失败时回滚", async () => {
    mockSystemTheme(false);
    const settings = application();
    const snapshot = {
      version: 1, theme: "system" as const,
      babysit: {
        provider: "codex" as const,
        codex: { executable: "C:\\codex.exe", model: null, reasoningEffort: null },
        claudeCode: { executable: "C:\\claude.exe", model: "sonnet", reasoningEffort: "high" },
      },
    };
    vi.mocked(settings.load).mockResolvedValue(snapshot);
    vi.mocked(settings.updateBabysitProvider).mockResolvedValue({ ...snapshot, babysit: { ...snapshot.babysit, provider: "claude-code" } });
    vi.mocked(settings.updateBabysitPreferences).mockRejectedValueOnce(new Error("原子替换失败"));
    render(<ApplicationSettingsDestination application={settings} initialSection="babysit" onBack={() => undefined} />);

    expect(await screen.findByTestId("codex-preferences")).toBeInTheDocument();
    expect(screen.queryByText("C:\\claude.exe")).not.toBeInTheDocument();
    await chooseSelect("使用的 Agent", "Claude Code");
    expect(await screen.findByTestId("claude-code-preferences")).toHaveTextContent("C:\\claude.exe");
    await chooseSelect("模型", "Haiku");

    await waitFor(() => expect(settings.updateBabysitPreferences).toHaveBeenCalledWith("claude-code", { executable: "C:\\claude.exe", model: "haiku", reasoningEffort: null }, null));
    expect(await screen.findByRole("alert")).toHaveTextContent("已恢复原设置");
    expect(screen.getByRole("combobox", { name: "模型" })).toHaveTextContent("Sonnet");
  });

  it("Claude Code 显示 Best 的完整说明", async () => {
    const settings = application();
    vi.mocked(settings.load).mockResolvedValue({ version: 1, theme: "system", babysit: { provider: "claude-code", codex: { executable: null, model: null, reasoningEffort: null }, claudeCode: { executable: null, model: null, reasoningEffort: null } } });
    render(<ApplicationSettingsDestination application={settings} initialSection="babysit" onBack={() => undefined} />);

    fireEvent.click(await screen.findByRole("combobox", { name: "模型" }));
    expect(await screen.findByRole("option", { name: "Best（使用最强的可用模型）" })).toBeInTheDocument();
  });

  it("Codex 目录加载、空结果与失败都保留已保存偏好", async () => {
    const settings = application();
    let rejectCatalog!: (reason: Error) => void;
    vi.mocked(settings.listBabysitModels).mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectCatalog = reject; }));
    render(<ApplicationSettingsDestination application={settings} initialSection="babysit" onBack={() => undefined} />);

    expect(await screen.findByRole("combobox", { name: "模型" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("正在读取 Codex 模型目录");
    await act(async () => rejectCatalog(new Error("目录不可用")));
    expect(await screen.findByRole("button", { name: "重试获取模型" })).toBeInTheDocument();
    expect(screen.getByText("已保留当前保存值。")).toBeInTheDocument();

    vi.mocked(settings.listBabysitModels).mockResolvedValueOnce({ provider: "codex", revision: "empty", models: [] });
    fireEvent.click(screen.getByRole("button", { name: "重试获取模型" }));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("未返回可用模型"));
  });

  it("Claude Code Custom 提供完整 effort 选择", async () => {
    const settings = application();
    vi.mocked(settings.load).mockResolvedValue({ version: 1, theme: "system", babysit: { provider: "claude-code", codex: { executable: null, model: null, reasoningEffort: null }, claudeCode: { executable: null, model: "vendor/custom-model", reasoningEffort: null } } });
    render(<ApplicationSettingsDestination application={settings} initialSection="babysit" onBack={() => undefined} />);

    expect(await screen.findByRole("textbox", { name: "自定义模型" })).toHaveValue("vendor/custom-model");
    fireEvent.click(screen.getByRole("combobox", { name: "思考强度" }));
    for (const effort of ["Default", "Low", "Medium", "High", "XHigh", "Max"]) expect(await screen.findByRole("option", { name: effort })).toBeInTheDocument();
  });

  it("Claude Code Haiku 只允许 Default effort", async () => {
    const settings = application();
    vi.mocked(settings.load).mockResolvedValue({ version: 1, theme: "system", babysit: { provider: "claude-code", codex: { executable: null, model: null, reasoningEffort: null }, claudeCode: { executable: null, model: "haiku", reasoningEffort: null } } });
    render(<ApplicationSettingsDestination application={settings} initialSection="babysit" onBack={() => undefined} />);

    const effort = await screen.findByRole("combobox", { name: "思考强度" });
    expect(effort).toHaveTextContent("Default");
    expect(effort).toBeDisabled();
  });

  it("切换 provider 往返时保留各自偏好", async () => {
    const settings = application();
    let babysit = { provider: "codex" as "codex" | "claude-code", codex: { executable: "C:\\codex.exe", model: "gpt-5.4", reasoningEffort: "high" }, claudeCode: { executable: "C:\\claude.exe", model: "sonnet", reasoningEffort: "medium" } };
    vi.mocked(settings.load).mockImplementation(async () => ({ version: 1, theme: "system", babysit }));
    vi.mocked(settings.updateBabysitProvider).mockImplementation(async (provider) => {
      babysit = { ...babysit, provider };
      return { version: 1, theme: "system", babysit };
    });
    render(<ApplicationSettingsDestination application={settings} initialSection="babysit" onBack={() => undefined} />);

    expect(await screen.findByTestId("codex-preferences")).toHaveTextContent("C:\\codex.exe");
    await chooseSelect("使用的 Agent", "Claude Code");
    expect(await screen.findByTestId("claude-code-preferences")).toHaveTextContent("C:\\claude.exe");
    expect(screen.getByRole("combobox", { name: "模型" })).toHaveTextContent("Sonnet");
    await chooseSelect("使用的 Agent", "Codex");
    expect(await screen.findByTestId("codex-preferences")).toHaveTextContent("C:\\codex.exe");
    expect(settings.updateBabysitProvider).toHaveBeenCalledTimes(2);
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

  it("在 GitHub 分区展示 lifecycle 提供的当前身份", async () => {
    mockSystemTheme(false);
    render(<ApplicationSettingsDestination application={application()} githubIdentity={githubIdentity()} onBack={() => undefined} />);

    fireEvent.click(await screen.findByRole("tab", { name: "GitHub" }));

    expect(screen.getByText("Monalisa")).toBeInTheDocument();
    expect(screen.getByText("@octocat")).toBeInTheDocument();
    const avatar = screen.getByLabelText("Monalisa 的 GitHub 头像");
    expect(avatar).toBeInTheDocument();
  });

  it("说明退出影响，取消不调用 lifecycle，确认才退出", async () => {
    mockSystemTheme(false);
    const identity = githubIdentity();
    render(<ApplicationSettingsDestination application={application()} githubIdentity={identity} onBack={() => undefined} />);
    fireEvent.click(await screen.findByRole("tab", { name: "GitHub" }));

    fireEvent.click(screen.getByRole("button", { name: "退出 GitHub" }));
    expect(screen.getByRole("dialog")).toHaveTextContent("授权凭据以及身份和头像缓存");
    expect(screen.getByRole("dialog")).toHaveTextContent("Repository、Review 与本地代码不会被删除");
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(identity.signOut).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "退出 GitHub" }));
    fireEvent.click(screen.getByRole("button", { name: "确认退出" }));
    await waitFor(() => expect(identity.signOut).toHaveBeenCalledTimes(1));
  });

  it("退出失败时保留身份、展示错误并通过同一 lifecycle 重试", async () => {
    mockSystemTheme(false);
    const identity = githubIdentity();
    identity.error = { scope: "github-identity", operation: "sign-out", detail: "凭据存储暂时不可用" };
    vi.mocked(identity.signOut).mockResolvedValue({ kind: "failed" });
    render(<ApplicationSettingsDestination application={application()} githubIdentity={identity} onBack={() => undefined} />);
    fireEvent.click(await screen.findByRole("tab", { name: "GitHub" }));

    expect(screen.getByText("Monalisa")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("凭据存储暂时不可用");
    fireEvent.click(screen.getByRole("button", { name: "重试退出" }));
    await waitFor(() => expect(identity.signOut).toHaveBeenCalledTimes(1));
    expect(screen.getByText("Monalisa")).toBeInTheDocument();
  });

  it("退出成功后提交 lifecycle 的已退出身份状态", async () => {
    mockSystemTheme(false);
    /** 用公开 binding 模拟真实 lifecycle 成功后提交新状态。 */
    function SuccessfulSignOutSettings() {
      const [signedIn, setSignedIn] = useState(true);
      const identity = githubIdentity();
      identity.status = { ...identity.status!, signedIn, identity: signedIn ? identity.status!.identity : null };
      identity.signOut = async () => {
        setSignedIn(false);
        return { kind: "signed-out" };
      };
      return <ApplicationSettingsDestination application={application()} githubIdentity={identity} initialSection="github" onBack={() => undefined} />;
    }
    render(<SuccessfulSignOutSettings />);

    fireEvent.click(await screen.findByRole("button", { name: "退出 GitHub" }));
    fireEvent.click(screen.getByRole("button", { name: "确认退出" }));

    await waitFor(() => expect(screen.queryByText("Monalisa")).not.toBeInTheDocument());
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
      finishSave = () => resolve({ version: 1, theme, babysit: { provider: "codex", codex: { executable: null, model: null, reasoningEffort: null }, claudeCode: { executable: null, model: null, reasoningEffort: null } } });
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

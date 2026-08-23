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
  return {
    load: vi.fn(async () => ({ version: 1, theme, codexExecutable: null, babysitModel: { kind: "automatic" } })),
    updateTheme: vi.fn(async (nextTheme) => ({ version: 1, theme: nextTheme, codexExecutable: null, babysitModel: { kind: "automatic" } })),
  };
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

  it("在 GitHub 分区展示 lifecycle 提供的当前身份", async () => {
    mockSystemTheme(false);
    render(<ApplicationSettingsDestination application={application()} githubIdentity={githubIdentity()} onBack={() => undefined} />);

    fireEvent.click(await screen.findByRole("tab", { name: "GitHub" }));

    expect(screen.getByText("Monalisa")).toBeInTheDocument();
    expect(screen.getByText("@octocat")).toBeInTheDocument();
    const avatar = screen.getByLabelText("Monalisa 的 GitHub 头像");
    expect(avatar).toBeInTheDocument();
    expect(avatar.querySelector("img")).toHaveAttribute("src", "/cached/octocat.png");
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

import { afterEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import { ApplicationSettingsDestination, SettingsPage } from "@/components/settings-page";
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
    chooseCodexExecutable: vi.fn(async () => ({ version: 1, theme, codexExecutable: "C:\\Program Files\\Codex\\codex.exe", babysitModel: { kind: "automatic" } })),
    clearCodexExecutable: vi.fn(async () => ({ version: 1, theme, codexExecutable: null, babysitModel: { kind: "automatic" } })),
    listBabysitModels: vi.fn(async () => ({ revision: "catalog-1", models: [
      { id: "gpt-5.4", name: "GPT-5.4", isDefault: true, supportedReasoningEfforts: ["low", "high"], defaultReasoningEffort: "high" },
      { id: "gpt-5.4-mini", name: "GPT-5.4 Mini", isDefault: false, supportedReasoningEfforts: ["low", "medium"], defaultReasoningEffort: "medium" },
    ] })),
    updateBabysitModel: vi.fn(async (babysitModel) => ({ version: 1, theme, codexExecutable: null, babysitModel })),
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
  it("仅在修改时获取模型目录，取消后完整保留保存偏好", async () => {
    mockSystemTheme(false);
    const settings = application();
    vi.mocked(settings.load).mockResolvedValue({ version: 1, theme: "system", codexExecutable: null, babysitModel: { kind: "explicit", modelId: "gpt-5.4", reasoningEffort: "high" } });
    render(<ApplicationSettingsDestination application={settings} initialSection="babysit" onBack={() => undefined} />);

    expect(await screen.findByText("gpt-5.4 · high")).toBeInTheDocument();
    expect(settings.listBabysitModels).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "修改模型偏好" }));
    await screen.findByRole("combobox", { name: "模型" });
    expect(settings.listBabysitModels).toHaveBeenCalledTimes(1);
    await chooseSelect("模型", /GPT-5.4 Mini/);
    fireEvent.click(screen.getByRole("button", { name: "取消模型修改" }));

    expect(screen.getByText("gpt-5.4 · high")).toBeInTheDocument();
    expect(settings.updateBabysitModel).not.toHaveBeenCalled();
  });

  it("切换模型重置为模型默认，并显式保存具体思考强度", async () => {
    mockSystemTheme(false);
    const settings = application();
    render(<ApplicationSettingsDestination application={settings} initialSection="babysit" onBack={() => undefined} />);
    await screen.findByText("自动（使用 App Server 默认值）");
    fireEvent.click(screen.getByRole("button", { name: "修改模型偏好" }));
    await screen.findByRole("combobox", { name: "模型" });
    await chooseSelect("模型", /GPT-5.4 Mini/);
    expect(screen.getByRole("combobox", { name: "思考强度" })).toHaveTextContent("模型默认（medium）");
    await chooseSelect("思考强度", "low");
    fireEvent.click(screen.getByRole("button", { name: "保存模型偏好" }));

    await waitFor(() => expect(settings.updateBabysitModel).toHaveBeenCalledWith({ kind: "explicit", modelId: "gpt-5.4-mini", reasoningEffort: "low" }, "catalog-1"));
    expect(await screen.findByText("gpt-5.4-mini · low")).toBeInTheDocument();
  });

  it("已保存模型从动态目录消失时仍可选择 automatic 并保存", async () => {
    mockSystemTheme(false);
    const settings = application();
    vi.mocked(settings.load).mockResolvedValue({ version: 1, theme: "system", codexExecutable: null, babysitModel: { kind: "explicit", modelId: "retired-model", reasoningEffort: "high" } });
    render(<ApplicationSettingsDestination application={settings} initialSection="babysit" onBack={() => undefined} />);

    expect(await screen.findByText("retired-model · high")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "修改模型偏好" }));

    const modelSelect = await screen.findByRole("combobox", { name: "模型" });
    expect(modelSelect).toHaveTextContent("retired-model（不可用）");
    await chooseSelect("模型", "自动（使用 App Server 默认值）");
    fireEvent.click(screen.getByRole("button", { name: "保存模型偏好" }));

    await waitFor(() => expect(settings.updateBabysitModel).toHaveBeenCalledWith({ kind: "automatic" }, "catalog-1"));
    expect(screen.getByText("自动（使用 App Server 默认值）")).toBeInTheDocument();
  });

  it("模型保存值读取失败时显示错误并允许重试", async () => {
    const settings = application();
    vi.mocked(settings.load)
      .mockRejectedValueOnce(new Error("settings.json 暂时不可读"))
      .mockResolvedValueOnce({ version: 1, theme: "system", codexExecutable: null, babysitModel: { kind: "explicit", modelId: "gpt-5.4", reasoningEffort: "high" } });
    render(<SettingsPage
      theme={{ preference: "system", loaded: true, saving: false, error: undefined, editorTheme: "light", choose: vi.fn() }}
      codex={{ executable: null, loaded: true, saving: false, error: undefined, choose: vi.fn(), clear: vi.fn() }}
      modelApplication={settings}
      initialSection="babysit"
      onBack={() => undefined}
    />);

    expect(await screen.findByRole("alert")).toHaveTextContent("settings.json 暂时不可读");
    fireEvent.click(screen.getByRole("button", { name: "重试读取模型设置" }));

    expect(await screen.findByText("gpt-5.4 · high")).toBeInTheDocument();
    expect(settings.load).toHaveBeenCalledTimes(2);
  });

  it("目录失败保留旧值并允许重试，保存失败也不覆盖旧值", async () => {
    mockSystemTheme(false);
    const settings = application();
    vi.mocked(settings.listBabysitModels).mockRejectedValueOnce({ code: "app_server_unavailable", detail: "App Server 无法启动" });
    vi.mocked(settings.updateBabysitModel).mockRejectedValueOnce("settings.json 无法原子替换");
    render(<ApplicationSettingsDestination application={settings} initialSection="babysit" onBack={() => undefined} />);
    await screen.findByText("自动（使用 App Server 默认值）");
    fireEvent.click(screen.getByRole("button", { name: "修改模型偏好" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("App Server 无法启动");
    fireEvent.click(screen.getByRole("button", { name: "重试获取模型" }));
    await screen.findByRole("combobox", { name: "模型" });
    await chooseSelect("模型", /GPT-5.4（服务端默认）/);
    fireEvent.click(screen.getByRole("button", { name: "保存模型偏好" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("settings.json 无法原子替换");
    fireEvent.click(screen.getByRole("button", { name: "取消模型修改" }));
    expect(screen.getByText("自动（使用 App Server 默认值）")).toBeInTheDocument();
  });
  it("通过约定的 Tauri commands 读取并更新完整 Application Settings", async () => {
    const invoke = vi.fn(async (_command: string, args: Record<string, unknown>) => ({
      version: 1, theme: args.theme ?? "system", codexExecutable: null, babysitModel: { kind: "automatic" },
    }));
    const adapter = createTauriApplicationSettingsAdapter({ invoke } as never, vi.fn(async () => "C:\\Tools\\codex.exe"));

    await adapter.load();
    await adapter.updateTheme("dark");
    await adapter.chooseCodexExecutable();
    await adapter.clearCodexExecutable();
    await adapter.listBabysitModels();
    await adapter.updateBabysitModel({ kind: "explicit", modelId: "gpt-5.4", reasoningEffort: null }, "catalog-1");

    expect(invoke).toHaveBeenNthCalledWith(1, "load_application_settings", {});
    expect(invoke).toHaveBeenNthCalledWith(2, "update_application_theme", { theme: "dark" });
    expect(invoke).toHaveBeenNthCalledWith(3, "update_application_codex_executable", { executable: "C:\\Tools\\codex.exe" });
    expect(invoke).toHaveBeenNthCalledWith(4, "clear_application_codex_executable", {});
    expect(invoke).toHaveBeenNthCalledWith(5, "list_babysit_models", {});
    expect(invoke).toHaveBeenNthCalledWith(6, "update_application_babysit_model", { preference: { kind: "explicit", modelId: "gpt-5.4", reasoningEffort: null }, catalogRevision: "catalog-1" });
  });

  it("typed adapter 不改写模型目录与结构化失败", async () => {
    const catalog = { revision: "catalog-custom", models: [{ id: "custom-model", name: "Custom Model", isDefault: false, supportedReasoningEfforts: ["ultra"], defaultReasoningEffort: "ultra" }] };
    const failure = { code: "catalog_unavailable", detail: "连接中断" };
    const invoke = vi.fn()
      .mockResolvedValueOnce(catalog)
      .mockRejectedValueOnce(failure);
    const adapter = createTauriApplicationSettingsAdapter({ invoke } as never);

    await expect(adapter.listBabysitModels()).resolves.toBe(catalog);
    await expect(adapter.updateBabysitModel({ kind: "automatic" }, "catalog-custom")).rejects.toBe(failure);
  });

  it("呈现服务端动态增加的思考强度而不依赖前端枚举", async () => {
    mockSystemTheme(false);
    const settings = application();
    vi.mocked(settings.listBabysitModels).mockResolvedValue({ revision: "catalog-2", models: [
      { id: "future-model", name: "Future Model", isDefault: true, supportedReasoningEfforts: ["ultra"], defaultReasoningEffort: "ultra" },
    ] });
    render(<ApplicationSettingsDestination application={settings} initialSection="babysit" onBack={() => undefined} />);
    fireEvent.click(await screen.findByRole("button", { name: "修改模型偏好" }));
    await screen.findByRole("combobox", { name: "模型" });
    await chooseSelect("模型", /Future Model/);

    const effortSelect = screen.getByRole("combobox", { name: "思考强度" });
    expect(effortSelect).toHaveTextContent("模型默认（ultra）");
    fireEvent.click(effortSelect);
    expect(await screen.findByRole("option", { name: "ultra" })).toBeInTheDocument();
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

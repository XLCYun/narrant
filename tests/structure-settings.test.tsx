import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import { ApplicationSettingsDestination } from "@/components/settings-page";
import { createTauriApplicationSettingsAdapter } from "@/infrastructure/tauri-application-settings-adapter";
import { createTauriTransport } from "@/infrastructure/tauri/transport";
import { i18n } from "@/lib/i18n";
import type { ApplicationSettings, RustDependencyScope } from "@/settings/application-settings";

beforeEach(async () => { await i18n.changeLanguage("zh-CN"); });
afterEach(() => cleanup());

/** 从真实 typed adapter 构造设置页面，持久化用内存快照替代。 */
function fixture() {
  let saved: ApplicationSettings = {
    version: 1, theme: "system", language: "zh-CN",
    structureReview: { rust: { dependencyScope: "standard-library" } },
    babysit: {
      provider: "custom", repositoryExploration: false, allNonEmptyLines: false,
      codex: { executable: null, model: null, reasoningEffort: null },
      claudeCode: { executable: null, model: null, reasoningEffort: null },
      custom: { protocol: "openai_chat_completions", baseUrl: "", model: "", effort: "provider_default" },
    },
  };
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "update_application_rust_dependency_scope") {
      saved = { ...saved, structureReview: { rust: { dependencyScope: args!.scope as RustDependencyScope } } };
    }
    return saved;
  });
  const application = createTauriApplicationSettingsAdapter(createTauriTransport(invoke));
  return { application, invoke, snapshot: () => saved };
}

describe("结构改动设置", () => {
  it("从设置导航进入，默认开启标准库，并通过 typed command 保存所有档位", async () => {
    const { application, invoke } = fixture();
    const view = render(<ApplicationSettingsDestination application={application} onBack={() => undefined} />);
    fireEvent.click(screen.getByRole("tab", { name: "结构改动" }));
    const standardLibrary = await screen.findByRole("radio", { name: "仓库代码 + 标准库（默认）" });
    await waitFor(() => expect(standardLibrary).not.toHaveAttribute("aria-disabled", "true"));
    expect(standardLibrary).toBeChecked();
    expect(standardLibrary).toHaveAccessibleDescription("允许解析标准库，禁用第三方依赖分析；第三方依赖相关的关系可能会缺失。");
    expect(screen.getByRole("combobox", { name: "编程语言" })).toHaveTextContent("Rust");
    for (const [scope, name] of [
      ["repository-only", "仅仓库代码"],
      ["all-dependencies", "仓库代码 + 标准库 + 第三方依赖"],
      ["standard-library", "仓库代码 + 标准库（默认）"],
    ] as const) {
      const radio = screen.getByRole("radio", { name });
      fireEvent.click(radio);
      await waitFor(() => expect(radio).toBeChecked());
      expect(invoke).toHaveBeenCalledWith("update_application_rust_dependency_scope", { scope });
    }
    fireEvent.click(screen.getByRole("radio", { name: "仅仓库代码" }));
    await waitFor(() => expect(screen.getByRole("radio", { name: "仅仓库代码" })).toBeChecked());
    view.unmount();
    render(<ApplicationSettingsDestination application={application} initialSection="structure-review" onBack={() => undefined} />);
    await waitFor(() => expect(screen.getByRole("radio", { name: "仅仓库代码" })).toBeChecked());
  });

  it("保存期间锁定选择；保存失败保留旧值并支持重试", async () => {
    const { application, snapshot } = fixture();
    let reject!: (error: Error) => void;
    const update = vi.spyOn(application, "updateRustDependencyScope").mockImplementationOnce(() => new Promise((_, failure) => { reject = failure; }));
    render(<ApplicationSettingsDestination application={application} initialSection="structure-review" onBack={() => undefined} />);
    const radio = screen.getByRole("radio", { name: "仅仓库代码" });
    await waitFor(() => expect(radio).not.toHaveAttribute("aria-disabled", "true"));
    fireEvent.click(radio);
    expect(radio).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("combobox", { name: "编程语言" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("正在保存");
    await act(async () => reject(new Error("disk unavailable")));
    expect(screen.getByRole("alert")).toHaveTextContent("已保留原选项");
    expect(screen.getByRole("radio", { name: "仓库代码 + 标准库（默认）" })).toBeChecked();
    expect(radio).not.toHaveAttribute("aria-disabled", "true");
    update.mockResolvedValueOnce({ ...snapshot(), structureReview: { rust: { dependencyScope: "repository-only" } } });
    fireEvent.click(radio);
    await waitFor(() => expect(radio).toBeChecked());
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("读取失败时不允许覆盖未知设置，可以重新读取", async () => {
    const { application, snapshot } = fixture();
    vi.spyOn(application, "load").mockRejectedValue(new Error("read failed"));
    render(<ApplicationSettingsDestination application={application} initialSection="structure-review" onBack={() => undefined} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("无法读取结构改动设置");
    expect(screen.getByRole("radio", { name: "仓库代码 + 标准库（默认）" })).toHaveAttribute("aria-disabled", "true");
    vi.mocked(application.load).mockResolvedValue(snapshot());
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    await waitFor(() => expect(screen.getByRole("radio", { name: "仓库代码 + 标准库（默认）" })).not.toHaveAttribute("aria-disabled", "true"));
  });
});

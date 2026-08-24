import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { GitHubIdentityEntry } from "@/components/github-identity-entry";
import { i18n } from "@/lib/i18n";

afterEach(() => {
  cleanup();
});

beforeEach(async () => {
  await i18n.changeLanguage("zh-CN");
});

describe("全局 GitHub Identity 入口", () => {
  it("未登录时从固定入口发起 GitHub 连接", () => {
    const onConnect = vi.fn();
    render(<GitHubIdentityEntry signedIn={false} onConnect={onConnect} onSignOut={() => undefined} />);

    const trigger = screen.getByRole("button", { name: "连接 GitHub" });
    expect(trigger.querySelector("svg")).toHaveClass("size-[22px]");
    fireEvent.click(trigger);

    expect(onConnect).toHaveBeenCalledOnce();
  });

  it("GitHub Identity 状态不可用时从同一入口重试", () => {
    const onConnect = vi.fn();
    const onRetry = vi.fn();
    render(<GitHubIdentityEntry
      signedIn={false}
      error="credential store unavailable"
      onConnect={onConnect}
      onSignOut={() => undefined}
      onRetry={onRetry}
    />);

    fireEvent.click(screen.getByRole("button", { name: "重试 GitHub 状态" }));

    expect(onRetry).toHaveBeenCalledOnce();
    expect(onConnect).not.toHaveBeenCalled();
  });

  it("显示 GitHub 名称，并在菜单中用 login 区分身份和退出", async () => {
    const onSignOut = vi.fn();
    render(<GitHubIdentityEntry
      signedIn
      identity={{ id: 42, login: "octocat", name: "Monalisa", avatarPath: null }}
      onConnect={() => undefined}
      onSignOut={onSignOut}
      className="h-auto min-h-12 w-full justify-start py-2"
    />);

    const trigger = screen.getByRole("button", { name: "Monalisa" });
    expect(trigger).toHaveClass("min-h-12", "py-2");
    expect(trigger.querySelector('[data-slot="avatar"]')).toHaveClass("size-9");
    fireEvent.click(trigger);

    const menu = await waitFor(() => {
      const element = screen.getByRole("menu");
      expect(element).toBeVisible();
      return element;
    });
    expect(menu).toHaveTextContent("@octocat");
    expect(menu).toHaveTextContent("MO");
    fireEvent.click(screen.getByRole("menuitem", { name: "退出 GitHub" }));
    expect(onSignOut).toHaveBeenCalledOnce();
  });

  it("缺少资料时仍明确显示已连接而不是登录失败", () => {
    render(<GitHubIdentityEntry signedIn onConnect={() => undefined} onSignOut={() => undefined} />);

    expect(screen.getByRole("button", { name: "GitHub 已连接" })).toBeVisible();
  });

  it("已登录身份的操作失败时显示 scoped error 并允许重试", () => {
    const onRetry = vi.fn();
    render(<GitHubIdentityEntry
      signedIn
      identity={{ id: 42, login: "octocat", name: "Monalisa", avatarPath: null }}
      error="credential store unavailable"
      onConnect={() => undefined}
      onSignOut={() => undefined}
      onRetry={onRetry}
    />);

    expect(screen.getByRole("alert")).toHaveTextContent("credential store unavailable");
    fireEvent.click(screen.getByRole("button", { name: "重试 GitHub 操作" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("显示名称为空时使用 login，并在头像解码失败后显示首字母", () => {
    render(<GitHubIdentityEntry
      signedIn
      identity={{ id: 42, login: "octocat", name: null, avatarPath: "/broken-avatar.png" }}
      onConnect={() => undefined}
      onSignOut={() => undefined}
    />);

    const trigger = screen.getByRole("button", { name: "octocat" });

    expect(trigger).toHaveTextContent("OC");
  });
});

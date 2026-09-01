import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";

import type { ApplicationUpdateSnapshot } from "@/application-update/application-update";
import type { ApplicationUpdateBinding } from "@/application-update/use-application-update";
import { ApplicationUpdateAttention } from "@/components/application-update/application-update-attention";
import { ApplicationUpdateSurface } from "@/components/application-update/application-update-surface";
import { ApplicationUpdateSettings } from "@/components/settings-page/application-update-settings";
import { i18n } from "@/lib/i18n";

afterEach(async () => {
  cleanup();
  await i18n.changeLanguage("zh-CN");
});

function snapshot(kind: ApplicationUpdateSnapshot["state"]["kind"]): ApplicationUpdateSnapshot {
  return {
    sequence: 2,
    currentVersion: "2.0.0",
    automaticDownload: true,
    state: kind === "downloading"
      ? { kind, progress: { kind: "known_total", downloadedBytes: 40, totalBytes: 100 } }
      : { kind },
    candidate: ["available", "downloading", "failed", "staged"].includes(kind)
      ? { version: "2.1.0", releaseNotes: "## Changes\n\n| Item | State |\n| --- | --- |\n| Update | Ready |" }
      : null,
    failure: kind === "failed" ? { operation: "download", category: "network", retryable: true, message: "private" } : null,
    recoveryWarning: null,
    preferenceWarning: null,
  };
}

function binding(value: ApplicationUpdateSnapshot, open = false): ApplicationUpdateBinding {
  return {
    snapshot: value,
    commandError: undefined,
    pending: undefined,
    surfaceOpen: open,
    openSurface: vi.fn(),
    setSurfaceOpen: vi.fn(),
    reload: vi.fn(),
    manualCheck: vi.fn(async () => undefined),
    setAutomaticDownload: vi.fn(async () => undefined),
    retry: vi.fn(async () => undefined),
    restart: vi.fn(async () => undefined),
  };
}

describe("Application Update UI", () => {
  it.each([
    ["idle", "等待检查"], ["checking", "正在检查"], ["up_to_date", "已是最新版本"],
    ["available", "有新版本可用"], ["downloading", "正在下载更新"],
    ["staged", "更新已准备就绪"], ["failed", "更新失败"],
  ] as const)("Settings 显示 %s 完整状态", (kind, label) => {
    render(<ApplicationUpdateSettings update={binding(snapshot(kind))} />);
    expect(screen.getByRole("status")).toHaveTextContent(label);
    expect(screen.getByText("2.0.0")).toBeVisible();
  });

  it.each(["available", "downloading", "failed", "staged"] as const)("%s 显示可访问 attention icon，点击只打开 surface", (kind) => {
    const onOpen = vi.fn();
    render(<ApplicationUpdateAttention snapshot={snapshot(kind)} onOpen={onOpen} />);
    fireEvent.click(screen.getByRole("button"));
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it("attention icon 使用原生 button 语义且点击只打开一次", () => {
    const onOpen = vi.fn();
    render(<ApplicationUpdateAttention snapshot={snapshot("staged")} onOpen={onOpen} />);
    const button = screen.getByRole("button");
    expect(button.tagName).toBe("BUTTON");
    fireEvent.click(button);
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it.each(["Enter", " "])("attention icon 用 %s 键只打开 surface 一次", (key) => {
    const onOpen = vi.fn();
    render(<ApplicationUpdateAttention snapshot={snapshot("staged")} onOpen={onOpen} />);
    fireEvent.keyDown(screen.getByRole("button"), { key });
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it.each(["idle", "checking", "up_to_date"] as const)("%s 不显示 attention icon", (kind) => {
    render(<ApplicationUpdateAttention snapshot={snapshot(kind)} onOpen={() => undefined} />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("已知下载长度显示确定百分比", () => {
    render(<ApplicationUpdateSurface update={binding(snapshot("downloading"), true)} />);
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "40");
    expect(screen.getByText(/40%/)).toBeVisible();
  });

  it("未知下载长度显示不确定进度和已下载字节，不伪造百分比", () => {
    const value = snapshot("downloading");
    value.state = { kind: "downloading", progress: { kind: "unknown_total", downloadedBytes: 2048 } };
    render(<ApplicationUpdateSurface update={binding(value, true)} />);
    expect(screen.getByRole("status", { name: "正在下载更新，文件总大小未知" })).toHaveTextContent("2.0 KB");
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  });

  it("安全呈现 GFM，丢弃原始 HTML、图片与危险链接", () => {
    const value = snapshot("available");
    value.candidate!.releaseNotes = "# Notes\n\n<table><tr><td>raw</td></tr></table>\n\n![tracking](https://evil.test/pixel)\n\n[unsafe](javascript:alert(1))\n\n| A | B |\n| - | - |\n| 1 | 2 |";
    render(<ApplicationUpdateSurface update={binding(value, true)} />);
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).queryByText("raw")).not.toBeInTheDocument();
    expect(within(dialog).getByText(/tracking/)).toBeVisible();
    expect(within(dialog).getByText("unsafe").tagName).toBe("SPAN");
    expect(within(dialog).getByRole("table")).toBeVisible();
    expect(dialog.querySelector("img")).toBeNull();
  });

  it("failed 只发送 retry intent，staged 才发送 restart intent", () => {
    const failed = binding(snapshot("failed"), true);
    const { unmount } = render(<ApplicationUpdateSurface update={failed} />);
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(failed.retry).toHaveBeenCalledOnce();
    expect(failed.restart).not.toHaveBeenCalled();
    unmount();

    const staged = binding(snapshot("staged"), true);
    render(<ApplicationUpdateSurface update={staged} />);
    fireEvent.click(screen.getByRole("button", { name: "立即重启并更新" }));
    expect(staged.restart).toHaveBeenCalledOnce();
  });

  it("en-US 使用自然、完整的更新文案", async () => {
    await i18n.changeLanguage("en");
    render(<ApplicationUpdateSurface update={binding(snapshot("staged"), true)} />);
    expect(screen.getByRole("dialog", { name: "Application update" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Restart and Update" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Later" })).toBeVisible();
  });
});

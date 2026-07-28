import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { HomePage } from "@/components/home-page";
import { i18n } from "@/lib/i18n";

afterEach(() => { cleanup(); window.localStorage.clear(); void i18n.changeLanguage("zh-CN"); });

describe("最近审核首页", () => {
  it("首次用户看到中文空状态和仓库文件夹入口", async () => {
    await i18n.changeLanguage("zh-CN");
    render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);
    expect(screen.getByRole("heading", { name: "纳言：审好代码，睡安稳觉" })).toBeVisible();
    expect(screen.getByRole("button", { name: "打开仓库文件夹" })).toBeVisible();
  });

  it("展示最多十条最近审核，并恢复已保存的侧边栏折叠偏好", async () => {
    window.localStorage.setItem("narrant.sidebar-preferences", JSON.stringify({ open: false }));
    const reviews = Array.from({ length: 11 }, (_, index) => ({ reviewId: `${index}`, repositoryEntryId: 1, repositoryPath: `/work/repo-${index}`, baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: index }));
    render(<HomePage reviews={reviews} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);
    expect(screen.getAllByRole("button", { name: /main → topic/ })).toHaveLength(20);
    fireEvent.click(screen.getByRole("button", { name: "展开侧边栏" }));
    expect(JSON.parse(window.localStorage.getItem("narrant.sidebar-preferences") ?? "{}")).toMatchObject({ open: true, width: 280 });
  });

  it("英文系统语言显示英文首页", async () => {
    await i18n.changeLanguage("en");
    render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);
    expect(screen.getByRole("heading", { name: "Narrant: review code, sleep soundly" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Open repository folder" })).toBeVisible();
  });
});

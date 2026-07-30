import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { HomePage } from "@/components/home-page";
import { i18n } from "@/lib/i18n";

const open = vi.hoisted(() => vi.fn());
const startDragging = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ startDragging }),
}));

afterEach(() => {
  cleanup();
  open.mockReset();
  startDragging.mockReset();
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  window.localStorage.clear();
  void i18n.changeLanguage("zh-CN");
});

describe("最近审核首页", () => {
  it("首次用户看到中文空状态和仓库文件夹入口", async () => {
    await i18n.changeLanguage("zh-CN");
    const { container } = render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);
    expect(screen.getByRole("img", { name: "纳言" })).toBeVisible();
    const darkLogo = container.querySelector<HTMLSourceElement>('source[media="(prefers-color-scheme: dark)"]');
    expect(darkLogo?.getAttribute("srcset")).toContain("%23F9FAFB");
    expect(screen.getAllByText("审好代码，睡安稳觉")).toHaveLength(2);
    expect(screen.getByRole("heading", { name: "纳言：审好代码，睡安稳觉" })).toBeVisible();
    expect(screen.getByRole("button", { name: "打开仓库文件夹" })).toBeVisible();
  });

  it("通过原生单目录选择器把所选路径交给仓库打开流程", async () => {
    await i18n.changeLanguage("zh-CN");
    open.mockResolvedValue("/work/narrant");
    const onOpenFolder = vi.fn();
    render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={onOpenFolder} />);

    fireEvent.click(screen.getByRole("button", { name: "打开仓库文件夹" }));

    await waitFor(() => expect(open).toHaveBeenCalledWith({ directory: true, multiple: false }));
    expect(onOpenFolder).toHaveBeenCalledWith("/work/narrant");
  });

  it("侧边栏默认展示五条最近审核，可展开到十条，并恢复折叠偏好", async () => {
    window.localStorage.setItem("narrant.sidebar-preferences", JSON.stringify({ open: false }));
    const reviews = Array.from({ length: 11 }, (_, index) => ({ reviewId: `${index}`, repositoryEntryId: 1, repositoryPath: `/work/repo-${index}`, baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: index }));
    render(<HomePage reviews={reviews} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);
    expect(screen.getAllByLabelText("将 Head 分支 topic 合入 Base 分支 main")).toHaveLength(15);
    fireEvent.click(screen.getByRole("button", { name: "查看全部" }));
    expect(screen.getAllByLabelText("将 Head 分支 topic 合入 Base 分支 main")).toHaveLength(20);
    const trigger = screen.getByRole("button", { name: "展开侧边栏" });
    const collapsedIcon = trigger.innerHTML;
    fireEvent.click(trigger);
    expect(screen.getByRole("button", { name: "收起侧边栏" }).innerHTML).not.toBe(collapsedIcon);
    expect(JSON.parse(window.localStorage.getItem("narrant.sidebar-preferences") ?? "{}")).toMatchObject({ open: true, width: 280 });
  });

  it("已有审核时不重复展示主体品牌，并把打开仓库入口放在最近审核标题行", () => {
    const reviews = [{ reviewId: "1", repositoryEntryId: 1, repositoryPath: "/work/repo", baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: 1 }];
    render(<HomePage reviews={reviews} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    const recent = screen.getByRole("region", { name: "最近审核" });
    expect(recent).toContainElement(screen.getByRole("heading", { name: "最近审核" }));
    expect(recent).toContainElement(screen.getByRole("button", { name: "打开仓库文件夹" }));
    expect(screen.queryByRole("heading", { name: "纳言" })).not.toBeInTheDocument();
  });

  it("拖动侧边栏时禁用宽度动画和文本选择，并让宽度立即跟随指针", () => {
    window.localStorage.setItem("narrant.sidebar-preferences", JSON.stringify({ open: true, width: 280 }));
    render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    const resizeHandle = screen.getByLabelText("调整侧边栏宽度");
    const wrapper = document.querySelector("[data-slot=sidebar-wrapper]");
    const titlebar = document.querySelector("[data-slot=window-titlebar]");
    expect(titlebar?.firstElementChild).toHaveClass("border-r", "border-sidebar-border");
    fireEvent.mouseDown(resizeHandle, { clientX: 280 });
    expect(wrapper).toHaveAttribute("data-resizing", "true");
    expect(wrapper).toHaveClass("select-none");

    fireEvent.mouseMove(document, { clientX: 240 });
    expect(wrapper).toHaveStyle({ "--sidebar-width": "240px" });

    fireEvent.mouseUp(document);
    expect(wrapper).not.toHaveAttribute("data-resizing");
  });

  it("48px 顶栏显式拖动原生窗口，并与原生窗口按钮保留间距", () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    const titlebar = document.querySelector("[data-slot=window-titlebar]");
    expect(titlebar).toHaveClass("h-12");
    fireEvent.mouseDown(titlebar!, { button: 0 });
    expect(startDragging).toHaveBeenCalledOnce();
    const trigger = screen.getByRole("button", { name: "展开侧边栏" });
    expect(trigger).toHaveClass("bg-transparent");
    expect(trigger).toHaveClass(
      "supports-[padding:max(0px)]:left-[max(0.75rem,env(titlebar-area-x,0px)+5.5rem)]",
    );
    fireEvent.mouseDown(trigger, { button: 0 });
    expect(startDragging).toHaveBeenCalledOnce();
  });

  it("仓库按最近活动排序并支持多个仓库独立展开", () => {
    const repositories = [
      { id: 1, canonicalPath: "/work/older", lastOpenedAt: 1, available: true },
      { id: 2, canonicalPath: "/work/newer", lastOpenedAt: 2, available: true },
    ];
    const reviews = repositories.map((repository) => ({
      reviewId: String(repository.id),
      repositoryEntryId: repository.id,
      repositoryPath: repository.canonicalPath,
      baseRef: "refs/heads/main",
      headRef: `refs/heads/topic-${repository.id}`,
      lastOpenedAt: repository.lastOpenedAt,
    }));
    const onOpenRepository = vi.fn();
    render(<HomePage reviews={reviews} repositories={repositories} onOpenReview={() => undefined} onOpenRepository={onOpenRepository} onOpenFolder={() => undefined} />);

    const repositoryButtons = screen.getAllByRole("button", { name: /^(newer|older)$/ });
    expect(repositoryButtons.map((button) => button.textContent)).toEqual(["newer", "older"]);
    fireEvent.click(screen.getByRole("button", { name: "查看全部 newer" }));
    fireEvent.click(screen.getByRole("button", { name: "查看全部 older" }));
    expect(screen.getAllByLabelText("将 Head 分支 topic-1 合入 Base 分支 main")).toHaveLength(3);
    expect(screen.getAllByLabelText("将 Head 分支 topic-2 合入 Base 分支 main")).toHaveLength(3);
    fireEvent.click(screen.getByRole("button", { name: "newer" }));
    expect(onOpenRepository).toHaveBeenCalledWith(repositories[1]);
    expect(screen.getByText("代码仓库")).toBeVisible();
  });

  it("英文系统语言显示英文首页", async () => {
    await i18n.changeLanguage("en");
    render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);
    expect(screen.getByRole("heading", { name: "Narrant: review code, sleep soundly" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Open repository folder" })).toBeVisible();
  });

  it("首次加载使用可访问的局部 Skeleton", async () => {
    render(<HomePage reviews={[]} loading onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    expect(screen.getByRole("status", { name: "正在加载最近审核" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "纳言：审好代码，睡安稳觉" })).not.toBeInTheDocument();
  });

  it("最近审核首页仍提供文件夹入口并显示本地化仓库错误", async () => {
    await i18n.changeLanguage("en");
    const reviews = [{ reviewId: "1", repositoryEntryId: 1, repositoryPath: "/work/repo", baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: 1 }];
    render(<HomePage reviews={reviews} error="The selected folder is not a Git repository." onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    expect(screen.getByRole("button", { name: "Open repository folder" })).toBeVisible();
    expect(screen.getByRole("alert")).toHaveTextContent("The selected folder is not a Git repository.");
  });
});

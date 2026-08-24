import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { AppShell, HomePage } from "@/components/home-page";
import { GitHubIdentityEntry } from "@/components/github-identity-entry";
import { i18n } from "@/lib/i18n";
import type { PullRequestMetadata, RecentReviewSummary } from "@/lib/review-types";

/** 用紧凑输入构造跨 Repository 的最近 Review 测试投影。 */
function recentReview(input: {
  reviewId: string; repositoryId: number; repositoryPath: string;
  baseRef: string; headRef: string; lastOpenedAt: number; pullRequest?: PullRequestMetadata;
}): RecentReviewSummary {
  return {
    repositoryId: input.repositoryId,
    repositoryPath: input.repositoryPath,
    review: {
      reviewId: input.reviewId,
      selection: input.pullRequest
        ? { kind: "pull-request", baseRef: input.baseRef, headRef: input.headRef, pullRequest: input.pullRequest }
        : { kind: "branch", baseRef: input.baseRef, headRef: input.headRef },
      currentSnapshot: { baseRevision: "base", comparisonBase: "merge-base", headRevision: "head" },
      lastOpenedAt: input.lastOpenedAt,
    },
  };
}

const open = vi.hoisted(() => vi.fn());
const startDragging = vi.hoisted(() => vi.fn());
const originalNavigatorPlatform = window.navigator.platform;
const originalUserAgentData = (window.navigator as Navigator & { userAgentData?: unknown }).userAgentData;
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
  Object.defineProperty(window.navigator, "platform", { configurable: true, value: originalNavigatorPlatform });
  Object.defineProperty(window.navigator, "userAgentData", { configurable: true, value: originalUserAgentData });
  void i18n.changeLanguage("zh-CN");
});

describe("最近审核首页", () => {
  it("侧边栏底部入口点击时不改变垂直位置", () => {
    render(<AppShell
      navigation={{
        destination: { kind: "home" },
        sidebar: { reviews: [], repositories: [], loading: false, busy: false },
        navigate: async () => ({ kind: "navigated" }),
        registerAndOpen: async () => ({ kind: "navigated" }),
      }}
      githubIdentityEntry={<GitHubIdentityEntry signedIn={false} onConnect={() => undefined} onSignOut={() => undefined} />}
    ><p>Home</p></AppShell>);

    for (const button of [
      screen.getByRole("button", { name: /^(连接 GitHub|Connect GitHub)$/ }),
      screen.getByRole("button", { name: "设置" }),
    ]) {
      expect(button.className).not.toContain("active:not-aria-[haspopup]:translate-y-px");
    }
  });

  it("首页中的品牌入口保留语义与悬停反馈，但不显示常驻选中背景", () => {
    render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    const brandButton = screen.getByRole("button", { name: /^(纳言|Narrant)$/ });
    expect(brandButton).toHaveAttribute("aria-current", "page");
    expect(brandButton).toHaveClass("cursor-pointer", "hover:bg-sidebar-accent", "focus-visible:ring-2");
    expect(brandButton).not.toHaveClass("bg-sidebar-accent");
  });

  it("Review 阅读器只选中所属仓库树中的 Review", async () => {
    const repository = { id: 1, canonicalPath: "/work/repo", lastOpenedAtMillis: 1, availability: { kind: "available" } };
    const review = recentReview({
      reviewId: "review-1",
      repositoryId: 1,
      repositoryPath: "/work/repo",
      baseRef: "refs/heads/main",
      headRef: "refs/heads/topic",
      lastOpenedAt: 1,
    });
    render(<AppShell
      navigation={{
        destination: { kind: "review", repositoryId: 1, reviewId: "review-1" },
        sidebar: {
          reviews: [review], repositories: [repository], loading: false, busy: false,
          activeRepositoryId: 1, activeReviewId: "review-1",
        },
        navigate: async () => ({ kind: "navigated" }),
        registerAndOpen: async () => ({ kind: "navigated" }),
      }}
    ><p>Review reader</p></AppShell>);

    await waitFor(() => expect(document.querySelectorAll('[data-review-id="review-1"]')).toHaveLength(2));
    const entries = [...document.querySelectorAll<HTMLElement>('[data-review-id="review-1"]')];
    expect(entries.filter((entry) => entry.getAttribute("aria-current") === "page")).toHaveLength(1);
    expect(entries[0]).not.toHaveAttribute("aria-current");
  });

  it("首次用户看到中文空状态和仓库文件夹入口", async () => {
    await i18n.changeLanguage("zh-CN");
    const { container } = render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);
    expect(screen.getByRole("img", { name: "纳言" })).toBeVisible();
    const darkLogo = container.querySelector<HTMLSourceElement>('source[media="(prefers-color-scheme: dark)"]');
    expect(darkLogo?.getAttribute("srcset")).toContain("narrant-logo-dark.svg");
    expect(darkLogo?.getAttribute("srcset")).not.toMatch(/^data:/);
    expect(screen.getAllByText("好工程，纳良言")).toHaveLength(3);
    expect(screen.getAllByRole("heading", { name: "纳言" })).toHaveLength(2);
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
    const reviews = Array.from({ length: 11 }, (_, index) => recentReview({ reviewId: `${index}`, repositoryId: 1, repositoryPath: `/work/repo-${index}`, baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: index }));
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

  it("分组标题整行可折叠内容，并持久化展开状态", () => {
    const repository = { id: 1, canonicalPath: "/work/repo", lastOpenedAtMillis: 1, availability: { kind: "available" } };
    const reviews = [recentReview({ reviewId: "1", repositoryId: 1, repositoryPath: "/work/repo", baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: 1 })];
    render(<HomePage reviews={reviews} repositories={[repository]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    const recentGroup = screen.getByRole("button", { name: "最近审核" });
    const repositoryGroup = screen.getByRole("button", { name: "代码仓库" });
    expect(recentGroup).toHaveAttribute("aria-expanded", "true");
    expect(repositoryGroup).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(recentGroup);
    fireEvent.click(repositoryGroup);

    expect(recentGroup).toHaveAttribute("aria-expanded", "false");
    expect(repositoryGroup).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("button", { name: "repo" })).not.toBeInTheDocument();
    expect(JSON.parse(window.localStorage.getItem("narrant.sidebar-preferences") ?? "{}")).toMatchObject({
      recentGroupOpen: false,
      repositoriesGroupOpen: false,
    });
  });

  it("最近审核列表文字与分组标题左对齐", () => {
    const reviews = [recentReview({
      reviewId: "1",
      repositoryId: 1,
      repositoryPath: "/work/repo",
      baseRef: "refs/heads/main",
      headRef: "refs/heads/topic",
      lastOpenedAt: 1,
    })];
    render(<HomePage reviews={reviews} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    const recentGroup = screen.getByRole("button", { name: "最近审核" }).closest("[data-sidebar=group]");
    expect(recentGroup?.querySelector("[data-sidebar=menu]")).not.toHaveClass("ml-5", "pl-2");
    const reviewButton = recentGroup
      ?.querySelector("[aria-label='将 Head 分支 topic 合入 Base 分支 main']")
      ?.closest("button");
    expect(reviewButton).toHaveClass("px-3");
  });

  it("已有审核时不重复展示主体品牌，并把打开仓库入口放在最近审核标题行", () => {
    const reviews = [recentReview({ reviewId: "1", repositoryId: 1, repositoryPath: "/work/repo", baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: 1 })];
    render(<HomePage reviews={reviews} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    const recent = screen.getByRole("region", { name: "最近审核" });
    expect(recent).toContainElement(screen.getByRole("heading", { name: "最近审核" }));
    expect(recent).toContainElement(screen.getByRole("button", { name: "打开仓库文件夹" }));
    expect(screen.queryByRole("heading", { name: "纳言" })).not.toBeInTheDocument();
  });

  it("主界面向侧边栏投射左向阴影以体现更高层级", () => {
    const { container } = render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    const main = container.querySelector("main");
    const shadow = container.querySelector("[data-slot=main-elevation-shadow]");
    expect(main).toHaveClass("relative", "z-20");
    expect(shadow).toHaveClass("fixed", "inset-y-0", "w-5", "bg-linear-to-r", "from-transparent", "to-black/[0.03]", "dark:to-black/[0.06]");
  });

  it("拖动侧边栏时禁用宽度动画和文本选择，并让宽度立即跟随指针", () => {
    window.localStorage.setItem("narrant.sidebar-preferences", JSON.stringify({ open: true, width: 280 }));
    render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    const resizeHandle = screen.getByLabelText("调整侧边栏宽度");
    const wrapper = document.querySelector("[data-slot=sidebar-wrapper]");
    const titlebar = document.querySelector("[data-slot=window-titlebar]");
    const shadow = document.querySelector("[data-slot=main-elevation-shadow]");
    expect(titlebar?.firstElementChild).toHaveClass("border-r", "border-sidebar-border");
    expect(shadow).toHaveStyle({ left: "280px" });
    fireEvent.mouseDown(resizeHandle, { clientX: 280 });
    expect(wrapper).toHaveAttribute("data-resizing", "true");
    expect(wrapper).toHaveClass("select-none");

    fireEvent.mouseMove(document, { clientX: 240 });
    expect(wrapper).toHaveStyle({ "--sidebar-width": "240px" });
    expect(shadow).toHaveStyle({ left: "240px" });

    fireEvent.mouseMove(document, { clientX: 700 });
    expect(wrapper).toHaveStyle({ "--sidebar-width": "700px" });

    fireEvent.mouseUp(document);
    expect(wrapper).not.toHaveAttribute("data-resizing");
  });

  it("macOS 的 48px 顶栏显式拖动原生窗口，并为交通灯按钮保留间距", () => {
    Object.defineProperty(window.navigator, "userAgentData", { configurable: true, value: { platform: "macOS" } });
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

  it("Windows 的侧边栏切换按钮贴近顶栏左侧", () => {
    Object.defineProperty(window.navigator, "userAgentData", { configurable: true, value: { platform: "Windows" } });
    render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    const trigger = screen.getByRole("button", { name: "展开侧边栏" });
    expect(trigger).toHaveClass("left-3");
    expect(trigger).not.toHaveClass(
      "supports-[padding:max(0px)]:left-[max(0.75rem,env(titlebar-area-x,0px)+5.5rem)]",
    );
  });

  it("仓库主体独立展开 Review 列表，右侧按钮打开仓库", async () => {
    await i18n.changeLanguage("zh-CN");
    const repositories = [
      { id: 1, canonicalPath: "/work/older", lastOpenedAtMillis: 1, availability: { kind: "available" } },
      { id: 2, canonicalPath: "/work/newer", lastOpenedAtMillis: 2, availability: { kind: "available" } },
    ];
    const reviews = repositories.map((repository) => recentReview({
      reviewId: String(repository.id),
      repositoryId: repository.id,
      repositoryPath: repository.canonicalPath,
      baseRef: "refs/heads/main",
      headRef: `refs/heads/topic-${repository.id}`,
      lastOpenedAt: repository.lastOpenedAtMillis,
    }));
    const onOpenRepository = vi.fn();
    render(<HomePage reviews={reviews} repositories={repositories} onOpenReview={() => undefined} onOpenRepository={onOpenRepository} onOpenFolder={() => undefined} />);

    const repositoryButtons = screen.getAllByRole("button", { name: /^(newer|older)$/ });
    expect(repositoryButtons.map((button) => button.textContent)).toEqual(["older", "newer"]);
    fireEvent.click(screen.getByRole("button", { name: "newer" }));
    fireEvent.click(screen.getByRole("button", { name: "older" }));
    expect(screen.getAllByLabelText("将 Head 分支 topic-1 合入 Base 分支 main")).toHaveLength(3);
    expect(screen.getAllByLabelText("将 Head 分支 topic-2 合入 Base 分支 main")).toHaveLength(3);
    expect(repositoryButtons[0]).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: "打开仓库 older" })).toHaveClass("pointer-events-auto", "opacity-100");
    const reviewList = repositoryButtons[0].closest("li")?.querySelector("ul");
    expect(reviewList).toHaveClass("ml-5", "w-auto", "pl-2");
    expect(reviewList).not.toHaveClass("border-l");
    fireEvent.click(screen.getByRole("button", { name: "打开仓库 newer" }));
    expect(onOpenRepository).toHaveBeenCalledWith(repositories[1]);
    expect(repositoryButtons[0]).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("代码仓库")).toBeVisible();
  });

  it("打开仓库期间仅把目标仓库的箭头替换为可访问的加载状态", () => {
    const repositories = [
      { id: 1, canonicalPath: "/work/first", lastOpenedAtMillis: 1, availability: { kind: "available" } },
      { id: 2, canonicalPath: "/work/second", lastOpenedAtMillis: 2, availability: { kind: "available" } },
    ];

    render(
      <HomePage
        reviews={[]}
        repositories={repositories}
        pendingRepositoryId={2}
        onOpenReview={() => undefined}
        onOpenRepository={() => undefined}
        onOpenFolder={() => undefined}
      />,
    );

    const pendingButton = screen.getByRole("button", { name: "正在打开仓库 second" });
    const idleButton = screen.getByRole("button", { name: "打开仓库 first" });
    expect(pendingButton).toBeDisabled();
    expect(pendingButton).toHaveAttribute("aria-busy", "true");
    expect(pendingButton.querySelector(".animate-spin")).toBeInTheDocument();
    expect(pendingButton.querySelector("[role='status']")).not.toBeInTheDocument();
    expect(idleButton).not.toBeDisabled();
    expect(idleButton.querySelector(".animate-spin")).not.toBeInTheDocument();
  });

  it("侧边栏把本地分支 Review 压缩为不含类型复述的单行", () => {
    const repository = { id: 1, canonicalPath: "/work/repo", lastOpenedAtMillis: 1, availability: { kind: "available" } };
    const reviews = [recentReview({
      reviewId: "1",
      repositoryId: 1,
      repositoryPath: repository.canonicalPath,
      baseRef: "refs/heads/main",
      headRef: "refs/heads/topic",
      lastOpenedAt: 1,
    })];
    const { container } = render(
      <HomePage
        reviews={reviews}
        repositories={[repository]}
        onOpenReview={() => undefined}
        onOpenFolder={() => undefined}
      />,
    );

    const sidebar = container.querySelector("[data-sidebar=sidebar]");
    const recentReviewButton = sidebar
      ?.querySelector("[aria-label='将 Head 分支 topic 合入 Base 分支 main']")
      ?.closest("button");
    expect(recentReviewButton).toBeVisible();
    expect(recentReviewButton).toHaveAttribute("data-base-ui-tooltip-trigger");
    expect(recentReviewButton).not.toHaveTextContent("本地分支");
    expect(recentReviewButton?.firstElementChild).not.toHaveClass("flex-col");
    expect(recentReviewButton).toHaveTextContent(/repo.*topic.*main/);
    expect(recentReviewButton?.querySelector("[data-slot='badge']")).toHaveTextContent("repo");
    const compactRelation = recentReviewButton?.querySelector("[aria-label='将 Head 分支 topic 合入 Base 分支 main']");
    expect(compactRelation?.firstElementChild).toHaveClass("min-w-0", "truncate");
    expect(compactRelation?.firstElementChild).not.toHaveClass("shrink-0");
    expect(recentReviewButton?.lastElementChild?.lastElementChild).toHaveClass("shrink-0");

    fireEvent.click(screen.getByRole("button", { name: "repo" }));
    const localReviewButtons = [...sidebar!.querySelectorAll("[aria-label='将 Head 分支 topic 合入 Base 分支 main']")]
      .map((relation) => relation.closest("button"));
    expect(localReviewButtons).toHaveLength(2);
    expect(localReviewButtons.every((button) => button?.hasAttribute("data-base-ui-tooltip-trigger"))).toBe(true);
  });

  it("侧边栏把 Pull Request Review 压缩为单行并提供整行信息卡触发器", () => {
    const openedAt = new Date("2026-07-30T08:15:00Z").getTime();
    const review = recentReview({
      reviewId: "1",
      repositoryId: 1,
      repositoryPath: "/work/narrant",
      baseRef: "refs/remotes/origin/main",
      headRef: "refs/remotes/origin/feature/compact-reviews",
      lastOpenedAt: openedAt,
      pullRequest: {
        owner: "octo-org",
        repository: "narrant",
        number: 42,
        title: "Compact review navigation",
        description: "不应进入 tooltip",
        authorUsername: "octocat",
        state: "open" as const,
        isDraft: true,
        webUrl: "https://github.com/octo-org/narrant/pull/42",
        baseRef: "main",
        headRef: "feature/compact-reviews",
        synchronizedAtMillis: 1,
      },
    });
    const { container } = render(
      <HomePage reviews={[review]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />,
    );

    const sidebar = container.querySelector("[data-sidebar=sidebar]");
    const reviewButton = [...sidebar!.querySelectorAll("button")]
      .find((button) => button.textContent?.includes("#42 Compact review navigation"));
    expect(reviewButton).toBeVisible();
    expect(reviewButton).toHaveAttribute("data-base-ui-tooltip-trigger");
    expect(reviewButton?.firstElementChild).not.toHaveClass("flex-col");
    expect(reviewButton).not.toHaveTextContent("main");
    expect(reviewButton).not.toHaveTextContent("feature/compact-reviews");
  });

  it("没有 Review 的仓库展开后显示空状态", () => {
    const repository = { id: 1, canonicalPath: "/work/empty", lastOpenedAtMillis: 1, availability: { kind: "unavailable", reason: "missing" } };
    const onOpenRepository = vi.fn();
    render(<HomePage reviews={[]} repositories={[repository]} onOpenReview={() => undefined} onOpenRepository={onOpenRepository} onOpenFolder={() => undefined} />);

    fireEvent.click(screen.getByRole("button", { name: "empty" }));
    expect(screen.getAllByText("还没有审核记录")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "打开仓库 empty" }));
    expect(onOpenRepository).toHaveBeenCalledWith(repository);
  });

  it("英文系统语言显示英文首页", async () => {
    await i18n.changeLanguage("en");
    render(<HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />);
    expect(screen.getAllByRole("heading", { name: "Narrant" })).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Open repository folder" })).toBeVisible();
  });

  it("首次加载使用可访问的局部 Skeleton", async () => {
    render(<HomePage reviews={[]} loading onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    expect(screen.getByRole("status", { name: "正在加载最近审核" })).toBeVisible();
    expect(screen.queryAllByRole("heading", { name: "纳言" })).toHaveLength(1);
  });

  it("最近审核首页仍提供文件夹入口并显示本地化仓库错误", async () => {
    await i18n.changeLanguage("en");
    const reviews = [recentReview({ reviewId: "1", repositoryId: 1, repositoryPath: "/work/repo", baseRef: "refs/heads/main", headRef: "refs/heads/topic", lastOpenedAt: 1 })];
    render(<HomePage reviews={reviews} error="The selected folder is not a Git repository." onOpenReview={() => undefined} onOpenFolder={() => undefined} />);

    expect(screen.getByRole("button", { name: "Open repository folder" })).toBeVisible();
    expect(screen.getByRole("alert")).toHaveTextContent("The selected folder is not a Git repository.");
  });
});

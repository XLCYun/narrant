import { execFileSync } from "node:child_process";
import { openFixtureRepository } from "./helpers/open-repository";
import { linkedWorktrees } from "./helpers/worktree-observation";

/** 返回 fixture 仓库当前注册的 worktree 数量，不依赖本地化输出。 */
function worktreeCount(repositoryPath: string) {
  return linkedWorktrees(repositoryPath).length;
}

describe("本地分支比较", () => {
  it("通过系统 WebView 应用当前系统主题", async () => {
    await browser.waitUntil(async () => {
      const theme = await browser.execute(() => ({
        systemDark: window.matchMedia("(prefers-color-scheme: dark)").matches,
        rootDark: document.documentElement.classList.contains("dark"),
      }));
      return theme.rootDark === theme.systemDark;
    });
  });

  it("通过真实 Tauri IPC 显示只读 File Change", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    await openFixtureRepository(repositoryPath);
    await $("[role='tab']=本地分支").click();
    const branchSelectors = await $$(`[role='combobox']`);
    await expect(branchSelectors[1]).toHaveText(expect.stringContaining("main"));
    await expect(branchSelectors[0]).toHaveText(expect.stringContaining("head-ref"));
    await $("button=开始审核").click();
    await expect($("[aria-label='审核阅读器']")).toBeDisplayed();
    await $("[role='tab']=文件改动").click();
    await $("[aria-label='文件树'] button").click();
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await expect($("[aria-label='File Change'] h3")).toHaveText(expect.stringContaining("welcome.txt"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-before", expect.stringContaining("旧内容"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-after", expect.stringContaining("新内容"));
  });

  it("从 Review 导航到 Overview 与 Home 后保留 current Workspace 并可重开", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    const retained = worktreeCount(repositoryPath);
    expect(retained).toBeGreaterThanOrEqual(2);
    await $("button=退出审核").click();
    await expect($("[aria-label='Repository']")).toBeDisplayed();
    expect(worktreeCount(repositoryPath)).toBe(retained);
    const brand = await $("button[aria-label='Narrant'], button[aria-label='纳言']");
    if (!(await brand.isExisting())) await $("[data-slot='sidebar-trigger']").click();
    await brand.waitForExist();
    await brand.click();
    const recent = await $("main [aria-label='最近审核'] ul button");
    await expect(recent).toBeDisplayed();
    await recent.click();

    await expect($("[aria-label='审核阅读器']")).toBeDisplayed();
    expect(worktreeCount(repositoryPath)).toBe(retained);
  });

  it("英文 900×600 核心旅程可进入仓库、继续审核、读取 Diff 并重开", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    execFileSync("git", ["commit", "--allow-empty", "-m", "e2e update"], { cwd: repositoryPath });
    // 先在完整侧边栏宽度完成真实 Repository 导航，再切到窄窗口验证 Review 布局。
    await browser.setWindowSize(1200, 800);
    await openFixtureRepository(repositoryPath, "en");
    await browser.setWindowSize(900, 600);
    await $("[role='tab']=Local branches").click();
    const branchSelectors = await $$(`[role='combobox']`);
    await expect(branchSelectors[1]).toBeDisplayed();
    await expect(branchSelectors[0]).toBeDisplayed();
    await browser.waitUntil(async () => {
      const update = await $("button=Update and continue review");
      const continueReview = await $("button=Continue review");
      const ready = async (button: WebdriverIO.Element) => await button.isExisting()
        && (await button.getAttribute("disabled")) === null;
      return await ready(update) || await ready(continueReview);
    });
    const updateAndContinue = await $("button=Update and continue review");
    if (await updateAndContinue.isExisting() && (await updateAndContinue.getAttribute("disabled")) === null) {
      await updateAndContinue.click();
    } else {
      await $("button=Continue review").click();
    }
    await browser.waitUntil(async () => (
      await $("[aria-label='Review reader']").isExisting()
      || await $("[role='alert']").isExisting()
    ), { timeout: 60000 });
    if (!(await $("[aria-label='Review reader']").isExisting())) {
      throw new Error(await $("[role='alert']").getText());
    }
    await $("[role='tab']=File Changes").click();
    await $("[aria-label='File tree'] button").click();
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-after", expect.stringContaining("新内容"));

    await $("button[aria-label='Exit review']").click();
    const brand = await $("button[aria-label='Narrant'], button[aria-label='纳言']");
    if (!(await brand.isExisting())) await $("[data-slot='sidebar-trigger']").click();
    await brand.waitForExist();
    await brand.click();
    const recent = await $("main [aria-label='Recent reviews'] ul button");
    await recent.click();
    await expect($("[aria-label='Review reader']")).toBeDisplayed();
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await browser.setWindowSize(1200, 800);
  });
});

import { execFileSync } from "node:child_process";
import { openFixtureRepository } from "./helpers/open-repository";
import { linkedWorktrees } from "./helpers/worktree-observation";

/** 返回 fixture 仓库当前注册的 worktree 数量，不依赖本地化输出。 */
function worktreeCount(repositoryPath: string) {
  return linkedWorktrees(repositoryPath).length;
}

/** 读取当前呈现的 durable Review identity。 */
async function activeReviewId() {
  for (const item of await $$("[data-review-id][aria-current='page']")) {
    const reviewId = await item.getAttribute("data-review-id");
    if (reviewId) return reviewId;
  }
  throw new Error("当前视图没有 active Review identity");
}

/** 通过真实 shadcn/Base UI Combobox 选择一个 canonical ref。 */
async function selectRef(label: "Base Ref" | "Head Ref", value: string) {
  const triggers = await $$("[role='combobox']");
  const trigger = triggers[label === "Head Ref" ? 0 : 1];
  if (!trigger) throw new Error(`分支选择器数量不足：${triggers.length}`);
  await trigger.waitForDisplayed();
  if (await trigger.isClickable()) {
    await trigger.click();
  } else {
    await browser.execute((index) => {
      const target = document.querySelectorAll<HTMLElement>("[role='combobox']")[index];
      if (!target) throw new Error(`没有可见分支选择器：${index}`);
      target.click();
    }, label === "Head Ref" ? 0 : 1);
  }
  const branch = value.split("/").at(-1) ?? value;
  const search = await $("input[aria-label^='搜索分支'], input[aria-label^='Search branches']");
  await search.waitForDisplayed();
  await search.setValue(branch);
  await browser.waitUntil(async () => {
    for (const option of await $$("[role='option']")) {
      if (await option.isDisplayed() && (await option.getText()).includes(branch)) return true;
    }
    return false;
  });
  for (const option of await $$("[role='option']")) {
    if (await option.isDisplayed() && (await option.getText()).includes(branch)) {
      await option.waitForClickable();
      await option.click();
      await browser.waitUntil(async () => (await trigger.getText()).includes(branch));
      return;
    }
  }
  throw new Error(`找不到 ${label}: ${value}`);
}

/** 退出当前 Reader 后，通过真实 lifecycle command 显式关闭 durable Review。 */
async function closeReview(locale: "zh-CN" | "en" = "zh-CN", knownReviewId?: string) {
  const reviewId = knownReviewId ?? await activeReviewId();
  const exit = await $(locale === "en" ? "button[aria-label='Exit review']" : "button[aria-label='退出审核']");
  await exit.waitForDisplayed();
  await browser.execute((label) => {
    const action = document.querySelector<HTMLButtonElement>(`button[aria-label='${label}']`);
    if (!action) throw new Error(`没有可见的退出审核动作：${label}`);
    action.click();
  }, locale === "en" ? "Exit review" : "退出审核");
  await expect($("[aria-label='Repository']")).toBeDisplayed();
  await browser.tauri.execute(
    ({ core }, currentReviewId) => core.invoke("close_review", { reviewId: currentReviewId }),
    reviewId,
  );
}

/** 从持久化 navigation 精确定位一个 Branch Selection 的 Review identity。 */
async function reviewIdForSelection(baseRef: string, headRef: string) {
  const navigation = await browser.tauri.execute(
    ({ core }) => core.invoke<{
      recentReviews: Array<{ review: { reviewId: string; selection: { kind: string; baseRef: string; headRef: string } } }>;
    }>("load_repository_navigation"),
  );
  const review = navigation.recentReviews.find((candidate) => (
    candidate.review.selection.kind === "branch"
    && candidate.review.selection.baseRef === baseRef
    && candidate.review.selection.headRef === headRef
  ));
  if (!review) throw new Error(`没有持久化 Branch Review：${baseRef} → ${headRef}`);
  return review.review.reviewId;
}

/** 从 Home 的 Recent 列表精确重开指定 Review。 */
async function reopenRecentReview(reviewId: string) {
  for (const item of await $$(`[data-review-id='${reviewId}']`)) {
    if (await item.isDisplayed()) {
      await item.waitForClickable();
      await item.click();
      return;
    }
  }
  throw new Error(`Recent 列表没有可见 Review：${reviewId}`);
}

/** 从 desktop sidebar 回到 Home，避开窄窗口 offcanvas 中的隐藏副本。 */
async function navigateHome() {
  const openVisibleBrand = async () => {
    for (const brand of await $$("button[aria-label='Narrant'], button[aria-label='纳言']")) {
      if (await brand.isDisplayed()) {
        await brand.waitForClickable();
        await brand.click();
        return true;
      }
    }
    return false;
  };
  if (!(await openVisibleBrand())) {
    for (const trigger of await $$('[data-slot="sidebar-trigger"]')) {
      if (!(await trigger.isDisplayed())) continue;
      await trigger.waitForClickable();
      await trigger.click();
      break;
    }
    await browser.waitUntil(openVisibleBrand);
  }
  await browser.waitUntil(async () => (
    await $("main [aria-label='最近审核'], main [aria-label='Recent reviews']").isDisplayed()
  ));
}

/** 精确触发当前可见的主内容按钮。 */
async function clickVisibleAction(text: string) {
  for (const button of await $$(`button=${text}`)) {
    await button.scrollIntoView({ block: "center", inline: "center" });
    if (!(await button.isDisplayed())) continue;
    if (await button.isClickable()) await button.click();
    else await browser.execute((label) => {
      const action = [...document.querySelectorAll<HTMLButtonElement>("button")]
        .find((candidate) => candidate.textContent?.trim() === label);
      if (!action) throw new Error(`没有可见操作：${label}`);
      action.click();
    }, text);
    return;
  }
  throw new Error(`没有可见操作：${text}`);
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
    const branchSelectors = await $$("[role='combobox']");
    await expect(branchSelectors[1]).toHaveText(expect.stringContaining("main"));
    await expect(branchSelectors[0]).toHaveText(expect.stringContaining("head-ref"));
    await $("button=开始审核").click();
    await expect($("[aria-label='审核阅读器']")).toBeDisplayed();
    // 没有刷新动作即表示 current Snapshot 已是最新版本，保留原“最新版本”业务覆盖。
    await expect($("button[aria-label='刷新审核']")).not.toExist();
    await $("[role='tab']=文件改动").click();
    await $("[aria-label='文件树'] button").click();
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await expect($("[aria-label='File Change'] h3")).toHaveText(expect.stringContaining("welcome.txt"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-before", expect.stringContaining("旧内容"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-after", expect.stringContaining("新内容"));
  });

  it("关闭后从最近列表重开 current Snapshot，并覆盖反向 refs 与空比较", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    const currentReviewId = await reviewIdForSelection("refs/heads/main", "refs/heads/head-ref");
    const retainedCount = worktreeCount(repositoryPath);
    expect(retainedCount).toBeGreaterThanOrEqual(2);

    await closeReview("zh-CN", currentReviewId);
    await browser.waitUntil(() => worktreeCount(repositoryPath) === retainedCount - 1);
    await navigateHome();
    await reopenRecentReview(currentReviewId);

    await expect($("[aria-label='审核阅读器']")).toBeDisplayed();
    await browser.waitUntil(() => worktreeCount(repositoryPath) === retainedCount);
    expect(worktreeCount(repositoryPath)).toBe(retainedCount);

    await closeReview("zh-CN", currentReviewId);
    await $("[role='tab']=本地分支").click();
    const branchSelectors = await $$("[role='combobox']");
    await expect(branchSelectors[1]).toHaveText(expect.stringContaining("main"));
    await selectRef("Base Ref", "refs/heads/head-ref");
    await selectRef("Head Ref", "refs/heads/base-ref");
    await expect(branchSelectors[1]).toHaveText(expect.stringContaining("head-ref"));
    await expect(branchSelectors[0]).toHaveText(expect.stringContaining("base-ref"));
    await $("button=开始审核").click();

    await expect($("[aria-label*='Head 分支 base-ref'][aria-label*='Base 分支 head-ref']")).toBeDisplayed();
    await $("[role='tab']=文件改动").click();
    await expect($("h3=没有 File Change")).toBeDisplayed();
    await browser.waitUntil(() => worktreeCount(repositoryPath) === retainedCount);
    expect(worktreeCount(repositoryPath)).toBe(retainedCount);

    const reverseReviewId = await reviewIdForSelection("refs/heads/head-ref", "refs/heads/base-ref");
    await closeReview("zh-CN", reverseReviewId);
    await browser.waitUntil(() => worktreeCount(repositoryPath) === retainedCount - 1);
    await navigateHome();
    await reopenRecentReview(reverseReviewId);
    await expect($("[aria-label='审核阅读器']")).toBeDisplayed();
    await closeReview("zh-CN", reverseReviewId);
    await browser.waitUntil(() => worktreeCount(repositoryPath) === retainedCount - 1);
    await navigateHome();
    await expect($(`[data-review-id='${reverseReviewId}']`)).toBeDisplayed();
  });

  it("英文 900×600 核心旅程可进入仓库、更新审核、读取 Diff 并重开", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    execFileSync("git", ["commit", "--allow-empty", "-m", "e2e update"], { cwd: repositoryPath });
    await browser.setWindowSize(1200, 800);
    await openFixtureRepository(repositoryPath, "en");
    await $("[role='tab']=Local branches").click();
    const branchSelectors = await $$("[role='combobox']");
    await expect(branchSelectors[1]).toBeDisplayed();
    await expect(branchSelectors[0]).toBeDisplayed();
    await selectRef("Base Ref", "refs/heads/main");
    await selectRef("Head Ref", "refs/heads/head-ref");
    await browser.waitUntil(async () => (
      await $("button=Update and continue review").isExisting()
      || await $("button=Continue existing review").isExisting()
    ));
    await browser.setWindowSize(900, 600);
    await expect(branchSelectors[1]).toBeDisplayed();
    await expect(branchSelectors[0]).toBeDisplayed();
    const updateAndContinue = await $("button=Update and continue review");
    const refreshedDuringOpen = await updateAndContinue.isExisting();
    await clickVisibleAction(refreshedDuringOpen ? "Update and continue review" : "Continue existing review");
    await expect($("[aria-label='Review reader']")).toBeDisplayed();
    if (!refreshedDuringOpen) {
      const refreshReview = await $("button[aria-label='Refresh review']");
      await expect(refreshReview).toBeDisplayed();
      await refreshReview.click();
    }
    await expect($("button[aria-label='Refresh review']")).not.toExist();
    const reviewId = await reviewIdForSelection("refs/heads/main", "refs/heads/head-ref");
    await $("[role='tab']=File Changes").click();
    await $("[aria-label='File tree'] button").click();
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-after", expect.stringContaining("新内容"));

    await closeReview("en", reviewId);
    await browser.setWindowSize(1200, 800);
    await navigateHome();
    await reopenRecentReview(reviewId);
    await expect($("[aria-label='Review reader']")).toBeDisplayed();
    await $("[role='tab']=File Changes").click();
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await browser.setWindowSize(1200, 800);
  });
});

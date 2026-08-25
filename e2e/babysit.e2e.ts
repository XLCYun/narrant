import { openFixtureRepository } from "./helpers/open-repository";
import { activeReviewId, selectRef } from "./helpers/review-navigation";

/** 通过真实 Tauri WebView 验证宝宝模式的首帧状态反馈。 */
describe("宝宝模式", () => {
  it("点击受支持的代码文件后立即显示生成状态", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    await openFixtureRepository(repositoryPath);
    await $("[role='tab']=本地分支").click();
    await selectRef("Base Ref", "refs/heads/base-ref");
    await selectRef("Head Ref", "refs/heads/head-ref");
    const startReview = await $("button=开始审核");
    await startReview.waitForClickable();
    await startReview.click();
    await expect($("[aria-label='File Change']")).toBeDisplayed();

    const codeFile = await $("button*=z-explain.ts");
    await codeFile.waitForClickable();
    await codeFile.click();
    await expect($("[aria-label='File Change'] h3")).toHaveText(expect.stringContaining("z-explain.ts"));

    const toggle = await $("button=宝宝模式");
    await browser.waitUntil(async () => await toggle.getAttribute("aria-disabled") === "false", {
      timeoutMsg: "宝宝模式资格检查没有确认当前文件可分析",
    });
    await toggle.waitForClickable();
    await toggle.click();
    await expect($("button=关闭宝宝模式")).toBeDisplayed();
    await browser.waitUntil(async () => {
      const geometry = await browser.execute(() => {
        const footer = document.querySelector<HTMLElement>("[data-testid='babysit-footer']");
        if (!footer) return null;
        const rect = footer.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom, viewportHeight: window.innerHeight };
      });
      return Boolean(geometry && geometry.top >= 0 && geometry.bottom <= geometry.viewportHeight);
    }, {
      timeout: 1000,
      timeoutMsg: "点击宝宝模式后 1 秒内没有在 viewport 内显示状态条",
    });
    await expect($("[data-testid='babysit-footer']")).toHaveText(expect.stringContaining("正在生成逐行解释"));
    await $("button=取消宝宝模式").click();
    const reviewId = await activeReviewId();
    const exitReview = await $("button[aria-label='退出审核']");
    await exitReview.waitForClickable();
    await exitReview.click();
    await expect($("[aria-label='Repository']")).toBeDisplayed();
    await browser.tauri.execute(
      ({ core }, currentReviewId) => core.invoke("close_review", { reviewId: currentReviewId }),
      reviewId,
    );
  });
});

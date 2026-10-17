import { openFixtureRepository } from "./helpers/open-repository";
import { reviewIdForSelection, selectRef } from "./helpers/review-navigation";
import { clickWebView } from "./helpers/webview-interaction";

/** 通过真实 Tauri WebView 验证宝宝模式的首帧状态反馈。 */
describe("宝宝模式", () => {
  it("点击受支持的代码文件后立即显示生成状态", async () => {
    // 使用本地挂起的响应观察生成与取消，不调用真实模型服务或读取日常凭据。
    await browser.tauri.execute(({ core }) => core.invoke("save_application_custom_connection", {
      connection: {
        protocol: "openai_chat_completions", baseUrl: "http://127.0.0.1:18472/model",
        apiKey: "e2e-placeholder", model: "e2e-pending", effort: "provider_default",
      },
    }));
    await browser.tauri.execute(({ core }) => core.invoke("update_application_babysit_provider", { provider: "custom" }));
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    await openFixtureRepository(repositoryPath);
    await clickWebView(await $("[role='tab']=本地分支"), "切换到本地分支失败");
    await selectRef("Base Ref", "refs/heads/base-ref");
    await selectRef("Head Ref", "refs/heads/head-ref");
    const startReview = await $("button=开始审核");
    await startReview.waitForClickable();
    await clickWebView(startReview, "开始审核失败");
    await expect($("[aria-label='File Change']")).toBeDisplayed();

    const codeFile = await $("button*=z-explain.ts");
    await codeFile.waitForClickable();
    await clickWebView(codeFile, "打开宝宝模式代码文件失败");
    await expect($("[aria-label='File Change'] h3")).toHaveText(expect.stringContaining("z-explain.ts"));

    const toggle = await $("button[aria-label='宝宝模式']");
    await browser.waitUntil(async () => await toggle.getAttribute("aria-disabled") === "false", {
      timeoutMsg: "宝宝模式资格检查没有确认当前文件可分析",
    });
    await toggle.waitForClickable();
    await clickWebView(toggle, "开启宝宝模式失败");
    await expect(toggle).toHaveAttribute("aria-pressed", "true");
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
    await browser.waitUntil(async () => (await (await fetch("http://127.0.0.1:18472/model/status")).json()).active > 0,
      { timeoutMsg: "宝宝模式没有请求本地模型夹具" });
    // 在 WebView 同一任务内查询并取消当前生成；缺少动作必须明确失败。
    await browser.execute(() => {
      const action = Array.from(document.querySelectorAll<HTMLButtonElement>("button"))
        .find((button) => button.textContent?.trim() === "取消宝宝模式");
      if (!action) throw new Error("未找到取消宝宝模式按钮");
      action.click();
    });
    // 断言用户可见的取消状态；HTTP 连接存活时间不能代表后端分析任务是否已经结束。
    await expect($("[data-testid='babysit-footer']")).not.toBeDisplayed();
    await expect(toggle).not.toHaveAttribute("aria-busy", "true");
    const reviewId = await reviewIdForSelection("refs/heads/base-ref", "refs/heads/head-ref");
    const exitReview = await $("button[aria-label='退出审核']");
    await exitReview.waitForDisplayed();
    // Windows embedded WDIO 会把离屏 sidebar layer 计入 hit-test；精确触发已验证可见的 Reader action。
    await browser.execute(() => {
      const action = document.querySelector<HTMLButtonElement>("button[aria-label='退出审核']");
      if (!action) throw new Error("没有可见的退出审核动作");
      action.click();
    });
    await expect($("[aria-label='Repository']")).toBeDisplayed();
    await browser.tauri.execute(
      ({ core }, currentReviewId) => core.invoke("close_review", { reviewId: currentReviewId }),
      reviewId,
    );
  });
});

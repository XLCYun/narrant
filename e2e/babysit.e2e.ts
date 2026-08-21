import { openFixtureRepository } from "./helpers/open-repository";

/** 通过真实 Tauri WebView 验证宝宝模式的首帧状态反馈。 */
describe("宝宝模式", () => {
  it("点击受支持的代码文件后立即显示生成状态", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    await openFixtureRepository(repositoryPath);
    await $("[role='tab']=本地分支").click();
    await $("button=开始审核").click();
    await expect($("[aria-label='File Change']")).toBeDisplayed();

    const codeFile = await $("button*=z-explain.ts");
    await codeFile.waitForClickable();
    await codeFile.click();
    await expect($("[aria-label='File Change'] h3")).toHaveText(expect.stringContaining("z-explain.ts"));

    const toggle = await $("button=宝宝模式");
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
  });
});

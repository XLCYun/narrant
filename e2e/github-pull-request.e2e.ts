import { openFixtureRepository } from "./helpers/open-repository";

describe("公开 GitHub Pull Request", () => {
  it("通过真实 Tauri IPC 打开固定快照并按需读取 File Change", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    await openFixtureRepository(repositoryPath);
    const pullRequest = $("button[aria-label*='#42']");
    await expect(pullRequest).toBeDisplayed();
    await pullRequest.click();

    await browser.waitUntil(async () => (
      await $("[aria-label='审核阅读器']").isExisting() || await $("[role='alert']").isExisting()
    ));
    if (await $("[role='alert']").isExisting()) {
      throw new Error(await $("[role='alert']").getText());
    }
    await expect($("h2*=真实 Tauri 审核")).toBeDisplayed();
    await $("[role='tab']=审核概览").click();
    await expect($("article")).toHaveText(expect.stringContaining("安全描述"));
    await expect($("article img")).not.toExist();
    await $("[role='tab']=文件改动").click();
    await $("[aria-label='文件树'] button").click();
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-before", expect.stringContaining("旧内容"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-after", expect.stringContaining("新内容"));
  });
});

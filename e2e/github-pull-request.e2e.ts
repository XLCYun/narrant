import { openFixtureRepository } from "./helpers/open-repository";

describe("公开 GitHub Pull Request", () => {
  it("通过真实 Tauri IPC 打开固定快照并按需读取 File Change", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    await openFixtureRepository(repositoryPath);
    await expect($("button*=#42")).toBeDisplayed();
    await $("button*=#42").click();

    await browser.waitUntil(async () => (
      await $("h2=真实 Tauri 审核").isExisting() || await $("[role='alert']").isExisting()
    ));
    if (await $("[role='alert']").isExisting()) {
      throw new Error(await $("[role='alert']").getText());
    }
    await expect($("h2=真实 Tauri 审核")).toBeDisplayed();
    await expect($("[aria-label='Pull Request 描述'] strong")).toHaveText("安全描述");
    await expect($("[aria-label='Pull Request 描述'] img")).not.toExist();
    await $("[role='treeitem']").click();
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-before", expect.stringContaining("旧内容"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-after", expect.stringContaining("新内容"));
  });
});

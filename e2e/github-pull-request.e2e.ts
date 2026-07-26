describe("公开 GitHub Pull Request", () => {
  it("通过真实 Tauri IPC 打开固定快照并按需读取 File Change", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    await $("[aria-label='Repository 路径']").setValue(repositoryPath);
    await $("button=添加 Repository").click();
    await $("[aria-label='Repository Entries'] button").click();
    await $("[aria-label='GitHub Pull Request URL']").setValue("https://github.com/octo-org/octo-repo/pull/42");
    await $("button=匿名打开").click();

    await expect($("h2=真实 Tauri 审核")).toBeDisplayed();
    await expect($("[aria-label='Pull Request 描述'] strong")).toHaveText("安全描述");
    await expect($("[aria-label='Pull Request 描述'] img")).not.toExist();
    await $("[role='treeitem']").click();
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-before", expect.stringContaining("old"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-after", expect.stringContaining("new"));
  });
});

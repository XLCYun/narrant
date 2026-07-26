describe("Repository Catalog 持久化", () => {
  it("通过真实 Tauri IPC 添加仓库并在应用重启后重新打开", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    if (await $("button=返回 Repository Entries").isExisting()) {
      await $("button=返回 Repository Entries").click();
    }
    await $("[aria-label='Repository 路径']").setValue(repositoryPath);
    await $("button=添加 Repository").click();
    await expect($("[aria-label='Repository Entries'] button")).toBeDisplayed();

    // reloadSession 会结束当前真实 Tauri 进程并启动同一测试构建的新进程。
    await browser.reloadSession();
    const savedEntry = await $("[aria-label='Repository Entries'] button");
    await expect(savedEntry).toBeDisplayed();
    await savedEntry.click();

    await expect($("[aria-label='Repository Entry']")).toBeDisplayed();
    await expect($("[aria-label='本地仓库路径']")).toHaveValue(
      expect.stringContaining("repository"),
    );
  });
});

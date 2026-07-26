describe("本地分支比较", () => {
  it("通过系统 WebView 应用当前系统主题", async () => {
    const theme = await browser.execute(() => ({
      systemDark: window.matchMedia("(prefers-color-scheme: dark)").matches,
      rootDark: document.documentElement.classList.contains("dark"),
    }));

    expect(theme.rootDark).toBe(theme.systemDark);
  });

  it("通过真实 Tauri IPC 显示只读 File Change", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    if (await $("button=返回 Repository Entries").isExisting()) {
      await $("button=返回 Repository Entries").click();
    }
    await $("[aria-label='Repository 路径']").setValue(repositoryPath);
    await $("button=添加 Repository").click();
    await $("[aria-label='Repository Entries'] button").click();
    await $("button=读取分支").click();
    await expect($("[aria-label='Base Ref']")).toHaveText("refs/heads/base-ref");
    await expect($("[aria-label='Head Ref']")).toHaveText("refs/heads/head-ref");
    await $("button=打开 Review Snapshot").click();
    await $("[role='treeitem']").click();
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await expect($("[aria-label='File Change'] h2")).toHaveText(expect.stringContaining("welcome.txt"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-before", expect.stringContaining("旧内容"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-after", expect.stringContaining("新内容"));
  });
});

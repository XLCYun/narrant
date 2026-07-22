describe("本地分支比较", () => {
  it("通过真实 Tauri IPC 显示只读 File Change", async () => {
    await $("[aria-label='本地仓库路径']").setValue(process.env.NARRANT_E2E_REPOSITORY!);
    await $("button=读取分支").click();
    await expect($("[aria-label='Base Ref']")).toHaveText("base-ref");
    await expect($("[aria-label='Head Ref']")).toHaveText("head-ref");
    await $("button=打开 Review Snapshot").click();
    await $("[role='treeitem']").click();
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await expect($("h2")).toHaveText(expect.stringContaining("welcome.txt"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-before", expect.stringContaining("旧内容"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-after", expect.stringContaining("新内容"));
  });
});

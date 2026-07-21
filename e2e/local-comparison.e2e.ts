describe("本地分支比较", () => {
  it("通过真实 Tauri IPC 显示只读 File Change", async () => {
    await $("[aria-label='测试仓库路径']").setValue(process.env.NARRANT_E2E_REPOSITORY!);
    await $("[aria-label='Base Ref']").setValue("base");
    await $("[aria-label='Head Ref']").setValue("head");
    await $("button=打开 File Change").click();
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await expect($("h2")).toHaveText(expect.stringContaining("welcome.txt"));
  });
});

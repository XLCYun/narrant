import { execFileSync } from "node:child_process";

/** 返回 fixture 仓库当前注册的 worktree 数量，不依赖本地化输出。 */
function worktreeCount(repositoryPath: string) {
  return execFileSync("git", ["worktree", "list", "--porcelain", "-z"], {
    cwd: repositoryPath,
    encoding: "utf8",
  }).split("\0").filter((field) => field.startsWith("worktree ")).length;
}

/** 通过真实 shadcn/Base UI Select 选择一个 canonical ref。 */
async function selectRef(label: "Base Ref" | "Head Ref", value: string) {
  await $(`[aria-label='${label}']`).click();
  await browser.waitUntil(async () => (await $$("[role='option']")).length > 0);
  const options = await $$("[role='option']");
  for (const option of options) {
    if ((await option.getText()).includes(value)) {
      await option.click();
      return;
    }
  }
  throw new Error(`找不到 ${label}: ${value}`);
}

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
    if (await $("button=我了解风险并信任此 Repository Entry").isExisting()) {
      await $("button=我了解风险并信任此 Repository Entry").click();
    }
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

  it("关闭后从最近列表重开 current Snapshot 且只保留一个 Review Workspace", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    await expect($("button=关闭 Review")).toBeDisplayed();
    expect(worktreeCount(repositoryPath)).toBe(2);

    await $("button=关闭 Review").click();
    await browser.waitUntil(() => worktreeCount(repositoryPath) === 1);
    await $("button=返回 Repository Entries").click();
    const recent = await $("[aria-label='Recent Reviews'] button");
    await expect(recent).toBeDisplayed();
    await recent.click();

    await expect($("[role='treeitem']")).toBeDisplayed();
    await browser.waitUntil(() => worktreeCount(repositoryPath) === 2);
    expect(worktreeCount(repositoryPath)).toBe(2);

    await $("button=读取分支").click();
    await expect($("[aria-label='Base Ref']")).toHaveText("refs/heads/base-ref");
    await selectRef("Base Ref", "refs/heads/head-ref");
    await selectRef("Head Ref", "refs/heads/base-ref");
    await $("button=打开 Review Snapshot").click();

    await browser.waitUntil(() => worktreeCount(repositoryPath) === 2);
    expect(worktreeCount(repositoryPath)).toBe(2);

  });
});

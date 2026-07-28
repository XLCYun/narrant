import { execFileSync } from "node:child_process";
import { openFixtureRepository } from "./helpers/open-repository";

/** 返回 fixture 仓库当前注册的 worktree 数量，不依赖本地化输出。 */
function worktreeCount(repositoryPath: string) {
  return execFileSync("git", ["worktree", "list", "--porcelain", "-z"], {
    cwd: repositoryPath,
    encoding: "utf8",
  }).split("\0").filter((field) => field.startsWith("worktree ")).length;
}

/** 通过真实 shadcn/Base UI Select 选择一个 canonical ref。 */
async function selectRef(label: "Base Ref" | "Head Ref", value: string) {
  const trigger = await $(`[aria-label='${label}']`);
  await trigger.click();
  await browser.waitUntil(async () => (await $$("[role='option']")).length > 0);
  const options = await $$("[role='option']");
  for (const option of options) {
    if (await option.isDisplayed() && (await option.getText()) === value) {
      await option.click();
      await expect(trigger).toHaveText(value);
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
    await openFixtureRepository(repositoryPath);
    await $("[role='tab']=本地分支").click();
    await expect($("[aria-label='Base Ref']")).toHaveText("refs/heads/main");
    await expect($("[aria-label='Head Ref']")).toHaveText("refs/heads/head-ref");
    await $("button=开始审核").click();
    await expect($("[aria-label='Branch Review 更新'] [role='status']")).toHaveText(
      "当前已是最新版本",
    );
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await expect($("[aria-label='File Change'] h3")).toHaveText(expect.stringContaining("welcome.txt"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-before", expect.stringContaining("旧内容"));
    await expect($("[aria-label='File Change']")).toHaveAttribute("data-after", expect.stringContaining("新内容"));
  });

  it("关闭后从最近列表重开 current Snapshot 且只保留一个 Review Workspace", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    await expect($("button=关闭审核")).toBeDisplayed();
    expect(worktreeCount(repositoryPath)).toBe(2);

    await $("button=关闭审核").click();
    await browser.waitUntil(() => worktreeCount(repositoryPath) === 1);
    await $("button=返回 Repository Entries").click();
    const recent = await $("main [aria-label='最近审核'] button");
    await expect(recent).toBeDisplayed();
    await recent.click();

    await expect($("[role='treeitem']")).toBeDisplayed();
    await browser.waitUntil(() => worktreeCount(repositoryPath) === 2);
    expect(worktreeCount(repositoryPath)).toBe(2);

    await $("button=关闭审核").click();
    await $("[role='tab']=本地分支").click();
    await expect($("[aria-label='Base Ref']")).toHaveText("refs/heads/main");
    await selectRef("Base Ref", "refs/heads/head-ref");
    await selectRef("Head Ref", "refs/heads/base-ref");
    await $("button=开始审核").click();

    await expect($("[aria-label='Branch Review 更新']")).toHaveText(
      expect.stringContaining("refs/heads/head-ref → refs/heads/base-ref"),
    );
    await expect($("h3=没有 File Change")).toBeDisplayed();
    await browser.waitUntil(() => worktreeCount(repositoryPath) === 2);
    expect(worktreeCount(repositoryPath)).toBe(2);

    await $("button=关闭审核").click();
    await browser.waitUntil(() => worktreeCount(repositoryPath) === 1);
    await $("button=返回 Repository Entries").click();
    await expect($("main [aria-label='最近审核'] button")).toBeDisplayed();
  });
});

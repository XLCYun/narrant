import { openFixtureRepository } from "./helpers/open-repository";
import {
  gitCommandsSince,
  gitTraceOffset,
  linkedWorktrees,
  workspaceCommands,
} from "./helpers/worktree-observation";

/** 通过真实 shadcn/Base UI Select 选择一个 canonical ref。 */
async function selectRef(label: "Base Ref" | "Head Ref", value: string) {
  const triggers = await $$(`[role='combobox']`);
  const trigger = triggers[label === "Head Ref" ? 0 : 1];
  if (!trigger) throw new Error(`分支选择器数量不足：${triggers.length}`);
  try {
    await trigger.waitForDisplayed();
  } catch (reason) {
    const body = await browser.execute(() => document.body.innerText);
    throw new Error(`${String(reason)}\n页面状态:\n${body}`, { cause: reason });
  }
  await trigger.click();
  await browser.waitUntil(async () => (await $$(`[role='option']`)).length > 0);
  const options = await $$(`[role='option']`);
  for (const option of options) {
    if (await option.isDisplayed() && (await option.getText()) === value) {
      await option.click();
      await browser.waitUntil(async () => (await trigger.getText()) === value);
      return;
    }
  }
  throw new Error(`找不到 ${label}: ${value}`);
}

/** 等待 Review Reader 完整呈现；Workspace 保留证据不依赖 Monaco 或 File Change 读取时序。 */
async function expectReader() {
  await expect($(`[aria-label='审核阅读器']`)).toBeDisplayed();
}

/** 从 active sidebar item 读取 durable Review identity。 */
async function activeReviewId() {
  const item = await $(`[data-review-id][aria-current='page']`);
  await expect(item).toBeDisplayed();
  const reviewId = await item.getAttribute("data-review-id");
  if (!reviewId) throw new Error("active Review 缺少 durable reviewId");
  return reviewId;
}

/** 从 Review Reader 返回 Repository Overview，但不调用 lifecycle close。 */
async function leaveToRepositoryOverview() {
  await $(`button=退出审核`).click();
  await expect($(`[aria-label='Repository']`)).toBeDisplayed();
}

/** 读取 trace 文件中的 workspace 命令，避免把观察自身的 git 计入证据。 */
function workspaceCommandsAfter(tracePath: string, offset: number) {
  return workspaceCommands(gitCommandsSince(tracePath, offset));
}

describe("进程生命周期 Open Review 真实应用验证", () => {
  it("A→B→A 保留两个 Workspace，Overview/Home 导航不释放，正常退出清理全部", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    const tracePath = process.env.NARRANT_E2E_GIT_TRACE!;
    await openFixtureRepository(repositoryPath);

    const initialWorktrees = linkedWorktrees(repositoryPath);
    await $(`[role='tab']=本地分支`).click();
    await selectRef("Base Ref", "retained-base-ref");
    await selectRef("Head Ref", "retained-head-ref");
    await $(`button=开始审核`).click();
    await expectReader();
    const reviewA = await activeReviewId();
    const afterA = linkedWorktrees(repositoryPath);
    expect(afterA).toHaveLength(initialWorktrees.length + 1);

    await leaveToRepositoryOverview();
    expect(linkedWorktrees(repositoryPath)).toEqual(afterA);

    await $(`[role='tab']=本地分支`).click();
    await selectRef("Base Ref", "retained-head-ref");
    await selectRef("Head Ref", "retained-base-ref");
    await $(`button=开始审核`).click();
    await expectReader();
    const reviewB = await activeReviewId();
    expect(reviewB).not.toBe(reviewA);
    const afterB = linkedWorktrees(repositoryPath);
    expect(afterB).toHaveLength(initialWorktrees.length + 2);

    // Repository Overview 与 Home 都只是 presentation navigation，两个 linked worktree 必须原样保留。
    await leaveToRepositoryOverview();
    expect(linkedWorktrees(repositoryPath)).toEqual(afterB);
    const brand = await $(`button[aria-label='Narrant'], button[aria-label='纳言']`);
    if (!(await brand.isExisting())) await $(`[data-slot='sidebar-trigger']`).click();
    await brand.waitForExist();
    await brand.click();
    await expect($(`main [aria-label='最近审核']`)).toBeDisplayed();
    expect(linkedWorktrees(repositoryPath)).toEqual(afterB);

    const beforeWarmSwitch = gitTraceOffset(tracePath);
    await $(`[data-review-id='${reviewA}']`).click();
    await expectReader();
    await browser.pause(250);
    expect(await activeReviewId()).toBe(reviewA);
    expect(linkedWorktrees(repositoryPath)).toEqual(afterB);
    expect(workspaceCommandsAfter(tracePath, beforeWarmSwitch)).toEqual([]);

    // 当前 Presented Review 的重复点击还必须是完整 no-op，不触发 reopen 或 Git workspace 工作。
    const beforeDuplicate = gitTraceOffset(tracePath);
    await $(`[data-review-id='${reviewA}']`).click();
    await browser.pause(150);
    expect(workspaceCommandsAfter(tracePath, beforeDuplicate)).toEqual([]);
    expect(linkedWorktrees(repositoryPath)).toEqual(afterB);

    // 通过真实 Tauri window close 触发 RunEvent::Exit，再释放 WebDriver session 验证正常退出 cleanup。
    // 应用退出会同时关闭 embedded WebDriver；屏蔽 runner 的二次 DELETE，避免把预期断连报成测试失败。
    // WDIO 将该方法定义为只读属性，使用 defineProperty 才能在当前 worker 中覆盖它。
    Object.defineProperty(browser, "deleteSession", {
      configurable: true,
      value: async () => undefined,
    });
    try {
      await browser.tauri.execute(({ core }) => core.invoke("plugin:window|close", { label: "main" }));
    } catch {
      // window close 会先结束 WebDriver transport，随后才返回 invoke 结果属于正常退出时序。
    }
    try { await browser.deleteSession(); } catch { /* 应用已退出时 WebDriver 可能提前断开。 */ }
    for (let attempt = 0; attempt < 50 && linkedWorktrees(repositoryPath).length !== 1; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    expect(linkedWorktrees(repositoryPath)).toHaveLength(1);
  });
});

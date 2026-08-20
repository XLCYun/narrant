import { realpathSync } from "node:fs";

import { openFixtureRepository } from "./helpers/open-repository";
import {
  gitInvocationsSinceStable,
  invocationTouchesWorkspace,
  linkedWorktrees,
  stableGitTraceOffset,
  workspaceMutationInvocations,
} from "./helpers/worktree-observation";

/** 通过真实 shadcn/Base UI Combobox 选择一个 canonical ref。 */
async function selectRef(label: "Base Ref" | "Head Ref", value: string) {
  const triggers = await $$("[role='combobox']");
  const trigger = triggers[label === "Head Ref" ? 0 : 1];
  if (!trigger) throw new Error(`分支选择器数量不足：${triggers.length}`);
  await trigger.waitForDisplayed();
  await trigger.waitForClickable();
  await trigger.click();
  const search = await $("input[aria-label^='搜索分支']");
  await search.waitForDisplayed();
  await search.setValue(value.split("/").at(-1) ?? value);
  await browser.waitUntil(async () => {
    for (const option of await $$("[role='option']")) {
      if (await option.isDisplayed() && (await option.getText()).includes(value)) return true;
    }
    return false;
  });
  for (const option of await $$("[role='option']")) {
    if (!(await option.isDisplayed()) || !(await option.getText()).includes(value)) continue;
    await option.click();
    break;
  }
  await browser.waitUntil(async () => (await trigger.getText()).includes(value));
}

/** 从可见 active sidebar item 读取 durable Review identity。 */
async function activeReviewId() {
  for (const item of await $$("[data-review-id][aria-current='page']")) {
    if (await item.isDisplayed()) {
      const reviewId = await item.getAttribute("data-review-id");
      if (reviewId) return reviewId;
    }
  }
  throw new Error("当前视图没有可见的 active Review");
}

/** 从持久化 navigation 精确定位一个 Branch Selection 的 durable identity。 */
async function reviewIdForSelection(baseRef: string, headRef: string) {
  const navigation = await browser.tauri.execute(
    ({ core }) => core.invoke<{
      recentReviews: Array<{ review: { reviewId: string; selection: { kind: string; baseRef: string; headRef: string } } }>;
    }>("load_repository_navigation"),
  );
  const review = navigation.recentReviews.find((candidate) => (
    candidate.review.selection.kind === "branch"
    && candidate.review.selection.baseRef.endsWith(baseRef)
    && candidate.review.selection.headRef.endsWith(headRef)
  ));
  if (!review) throw new Error(`没有持久化 Branch Review：${baseRef} → ${headRef}`);
  return review.review.reviewId;
}

/** 等待目标 Review 完成切换且 Reader 可用。 */
async function waitForActiveReview(reviewId: string) {
  await browser.waitUntil(async () => {
    if (!(await $("[aria-label='审核阅读器']").isDisplayed())) return false;
    try {
      return await activeReviewId() === reviewId;
    } catch {
      return false;
    }
  });
}

/** 只点击当前可见、可交互的 Review sidebar item。 */
async function clickVisibleReview(reviewId: string) {
  let clicked = false;
  await browser.waitUntil(async () => {
    for (const item of await $$(`[data-review-id='${reviewId}']`)) {
      if (!(await item.isDisplayed()) || !(await item.isClickable())) continue;
      await item.click();
      clicked = true;
      return true;
    }
    return false;
  }, { timeoutMsg: `侧边栏没有可见且可交互的 Review：${reviewId}` });
  if (!clicked) throw new Error(`侧边栏 Review 点击未完成：${reviewId}`);
}

/** 确保 offcanvas Sidebar 展开，并返回可见品牌按钮。 */
async function visibleBrandButton() {
  for (const brand of await $$("button[aria-label='Narrant'], button[aria-label='纳言']")) {
    if (await brand.isDisplayed()) return brand;
  }
  for (const trigger of await $$("[data-slot='sidebar-trigger']")) {
    if (await trigger.isDisplayed()) {
      await trigger.waitForClickable();
      await trigger.click();
      break;
    }
  }
  await browser.waitUntil(async () => {
    for (const brand of await $$("button[aria-label='Narrant'], button[aria-label='纳言']")) {
      if (await brand.isDisplayed()) return true;
    }
    return false;
  });
  for (const brand of await $$("button[aria-label='Narrant'], button[aria-label='纳言']")) {
    if (await brand.isDisplayed()) return brand;
  }
  throw new Error("Sidebar 展开后仍没有可见品牌按钮");
}

/** 从宽窗口 Review Reader 返回 Repository Overview，但不显式关闭 Review。 */
async function leaveToRepositoryOverview() {
  const exit = await $("button[aria-label='退出审核']");
  await exit.waitForDisplayed();
  // embedded WDI0 会把已离屏的 sidebar layer 计入 hit-test；精确触发可见 Reader action。
  await browser.execute(() => {
    const action = document.querySelector<HTMLButtonElement>("button[aria-label='退出审核']");
    if (!action) throw new Error("没有可见的退出审核动作");
    action.click();
  });
  await expect($("[aria-label='Repository']")).toBeDisplayed();
}

/** 等待 linked worktree 精确达到期望集合。 */
async function waitForWorktrees(repositoryPath: string, expected: string[]) {
  await browser.waitUntil(() => {
    const actual = linkedWorktrees(repositoryPath);
    return actual.length === expected.length && actual.every((path, index) => path === expected[index]);
  });
}

/** 返回新增的唯一 linked worktree，缺失或新增多个都视为证据失败。 */
function addedWorkspace(before: string[], after: string[]) {
  const added = after.filter((path) => !before.includes(path));
  expect(added).toHaveLength(1);
  return added[0];
}

describe("进程生命周期 Open Review 真实应用验证", () => {
  it("A→B→A 保留两个 Workspace，Overview/Home 导航不释放，正常退出清理全部", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    const tracePath = process.env.NARRANT_E2E_GIT_TRACE!;
    await browser.setWindowSize(1400, 900);
    await openFixtureRepository(repositoryPath);

    const initialWorktrees = linkedWorktrees(repositoryPath);
    await $("[role='tab']=本地分支").click();
    await selectRef("Base Ref", "retained-base-ref");
    await selectRef("Head Ref", "retained-head-ref");
    await $("button=开始审核").click();
    await expect($("[aria-label='审核阅读器']")).toBeDisplayed();
    const reviewA = await reviewIdForSelection("retained-base-ref", "retained-head-ref");
    await $("[role='tab']=文件改动").click();
    await $("[aria-label='文件树'] button").click();
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    const afterA = linkedWorktrees(repositoryPath);
    expect(afterA).toHaveLength(initialWorktrees.length + 1);
    const workspaceA = addedWorkspace(initialWorktrees, afterA);
    await stableGitTraceOffset(tracePath);

    await leaveToRepositoryOverview();
    await waitForWorktrees(repositoryPath, afterA);

    const beforeB = await stableGitTraceOffset(tracePath);
    await $("[role='tab']=本地分支").click();
    await selectRef("Base Ref", "retained-head-ref");
    await selectRef("Head Ref", "retained-base-ref");
    await $("button=开始审核").click();
    await expect($("[aria-label='审核阅读器']")).toBeDisplayed();
    const reviewB = await reviewIdForSelection("retained-head-ref", "retained-base-ref");
    expect(reviewB).not.toBe(reviewA);
    const afterB = linkedWorktrees(repositoryPath);
    expect(afterB).toHaveLength(initialWorktrees.length + 2);
    expect(afterB).toContain(workspaceA);
    const workspaceB = addedWorkspace(afterA, afterB);
    const bInvocations = await gitInvocationsSinceStable(tracePath, beforeB);
    expect(bInvocations.length).toBeGreaterThan(0);
    expect(bInvocations.filter((invocation) => invocationTouchesWorkspace(invocation, workspaceA))).toEqual([]);
    const bMutations = workspaceMutationInvocations(bInvocations);
    if (!bMutations.some(({ argv }) => argv.includes("worktree") && argv.includes("add") && argv.some((argument) => {
      try { return realpathSync(argument) === realpathSync(workspaceB); } catch { return false; }
    }))) {
      throw new Error(`没有观测到 B Workspace 的 worktree add：${JSON.stringify({ workspaceB, bMutations })}`);
    }

    // B→A→B 的直接 warm switch 允许 Repository 只读检查，但不得重建、重置或释放 Workspace。
    await visibleBrandButton();
    const beforeWarmA = await stableGitTraceOffset(tracePath);
    await clickVisibleReview(reviewA);
    await waitForActiveReview(reviewA);
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await waitForWorktrees(repositoryPath, afterB);
    expect(workspaceMutationInvocations(await gitInvocationsSinceStable(tracePath, beforeWarmA))).toEqual([]);
    expect(linkedWorktrees(repositoryPath)).toContain(workspaceA);
    expect(linkedWorktrees(repositoryPath)).toContain(workspaceB);

    await visibleBrandButton();
    const beforeWarmB = await stableGitTraceOffset(tracePath);
    await clickVisibleReview(reviewB);
    await waitForActiveReview(reviewB);
    await waitForWorktrees(repositoryPath, afterB);
    expect(workspaceMutationInvocations(await gitInvocationsSinceStable(tracePath, beforeWarmB))).toEqual([]);
    expect(linkedWorktrees(repositoryPath)).toContain(workspaceA);
    expect(linkedWorktrees(repositoryPath)).toContain(workspaceB);

    // Repository Overview 与 Home 都只是 presentation navigation，两个 linked worktree 必须原样保留。
    await leaveToRepositoryOverview();
    await waitForWorktrees(repositoryPath, afterB);
    const brand = await visibleBrandButton();
    await brand.waitForClickable();
    await brand.click();
    await expect($("main [aria-label='最近审核']")).toBeDisplayed();
    await waitForWorktrees(repositoryPath, afterB);

    // 从 Home 恢复允许重新读取 Repository Overview，但不得物化、重置或释放任何 Workspace。
    const beforeHomeRestore = await stableGitTraceOffset(tracePath);
    await clickVisibleReview(reviewA);
    await waitForActiveReview(reviewA);
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await waitForWorktrees(repositoryPath, afterB);
    expect(workspaceMutationInvocations(await gitInvocationsSinceStable(tracePath, beforeHomeRestore))).toEqual([]);

    // 当前 Presented Review 的重复点击也必须是完整 no-op，不触发 reopen、内容读取或任何 Git invocation。
    await visibleBrandButton();
    const beforeDuplicate = await stableGitTraceOffset(tracePath);
    await clickVisibleReview(reviewA);
    await waitForActiveReview(reviewA);
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    await waitForWorktrees(repositoryPath, afterB);
    expect(await gitInvocationsSinceStable(tracePath, beforeDuplicate)).toEqual([]);

    // 通过真实 Tauri window close 触发 RunEvent::Exit，再验证正常退出 cleanup。
    Object.defineProperty(browser, "deleteSession", {
      configurable: true,
      value: async () => undefined,
    });
    try {
      await browser.tauri.execute(({ core }) => core.invoke("plugin:window|close", { label: "main" }));
    } catch {
      // window close 会先结束 WebDriver transport，随后才返回 invoke 结果，属于正常退出时序。
    }
    for (let attempt = 0; attempt < 50 && linkedWorktrees(repositoryPath).length !== 1; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    expect(linkedWorktrees(repositoryPath)).toHaveLength(1);
  });
});

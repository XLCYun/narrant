import { existsSync, realpathSync, statSync } from "node:fs";

import { openFixtureRepository } from "./helpers/open-repository";
import { activeReviewId, reviewIdForSelection, selectRef, visibleBrandButton } from "./helpers/review-navigation";
import {
  gitInvocationsSinceStable,
  invocationTouchesWorkspace,
  linkedWorktrees,
  stableGitTraceOffset,
  workspaceMutationInvocations,
  workspaceOperationInvocations,
} from "./helpers/worktree-observation";

/** 等待目标 Review 完成切换且 Reader 可用。 */
async function waitForActiveReview(reviewId: string) {
  try {
    await browser.waitUntil(async () => {
      if (!(await $("[aria-label='审核阅读器']").isDisplayed())) return false;
      try {
        return await activeReviewId() === reviewId;
      } catch {
        return false;
      }
    });
  } catch (reason) {
    const state = await browser.execute(() => ({
      reader: Boolean(document.querySelector("[aria-label='审核阅读器']")),
      active: [...document.querySelectorAll<HTMLElement>("[data-review-id][aria-current='page']")]
        .map((item) => ({ id: item.dataset.reviewId, visible: item.offsetParent !== null })),
      alerts: [...document.querySelectorAll<HTMLElement>("[role='alert']")].map((item) => item.textContent?.trim()),
    }));
    throw new Error(`${String(reason)}；active Review state：${JSON.stringify(state)}`, { cause: reason });
  }
}

/** 只点击当前可见、可交互的 Review sidebar item。 */
async function clickVisibleReview(reviewId: string) {
  let openedSidebar = false;
  await browser.waitUntil(async () => {
    for (const item of await $$(`[data-review-id='${reviewId}']`)) {
      await item.scrollIntoView({ block: "center", inline: "nearest" });
      if (!(await item.isDisplayed())) continue;
      await item.waitForDisplayed();
      if (await item.isClickable()) await item.click();
      else {
        // embedded WDI0 的离屏 sidebar layer 会干扰 hit-test；只对已验证可见的精确按钮触发产品 click。
        await browser.execute((button) => button.click(), item);
      }
      return true;
    }
    if (!openedSidebar) {
      for (const trigger of await $$("[data-slot='sidebar-trigger']")) {
        if (!(await trigger.isDisplayed()) || !(await trigger.isClickable())) continue;
        await trigger.waitForClickable();
        await trigger.click();
        openedSidebar = true;
        break;
      }
    }
    return false;
  }, { timeoutMsg: `侧边栏没有可见 Review：${reviewId}` });
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

/** 读取 SQLite 主文件与 WAL 的稳定文件指纹，用于证明重复点击没有持久化写入。 */
function persistenceFingerprint(databasePath: string) {
  return [databasePath, `${databasePath}-wal`].filter(existsSync).map((path) => {
    const stat = statSync(path);
    return { path, size: stat.size, mtimeMs: stat.mtimeMs };
  });
}

/** 读取 durable Review 的 snapshot 与 last-opened，用于比较重复点击前后的持久化状态。 */
async function persistedReview(reviewId: string) {
  const navigation = await browser.tauri.execute(
    ({ core }) => core.invoke<{ recentReviews: Array<{ review: { reviewId: string; currentSnapshot: unknown; lastOpenedAt: number } }> }>("load_repository_navigation"),
  );
  const review = navigation.recentReviews.find((candidate) => candidate.review.reviewId === reviewId)?.review;
  if (!review) throw new Error(`navigation 中没有 Review：${reviewId}`);
  return review;
}

/** 捕获用户可见 Reader 的身份、布局与滚动状态。 */
async function readerEvidence() {
  return browser.execute(() => {
    const reader = document.querySelector<HTMLElement>("[aria-label='审核阅读器']");
    const fileChange = document.querySelector<HTMLElement>("[aria-label='File Change']");
    const fileTree = reader?.querySelector<HTMLElement>("nav[aria-label='文件树'], nav[aria-label='File tree']");
    const activeReview = [...document.querySelectorAll<HTMLElement>("[data-review-id][aria-current='page']")]
      .find((item) => item.offsetParent !== null);
    const activeTab = reader?.querySelector<HTMLElement>("[role='tab'][aria-selected='true']");
    const selectedFile = fileTree?.querySelector<HTMLElement>("button[aria-current='true']");
    const recentHeading = [...document.querySelectorAll<HTMLButtonElement>("[data-sidebar='group'] > button")]
      .find((button) => ["最近审核", "Recent reviews"].includes(button.textContent?.trim() ?? "") && button.offsetParent !== null);
    const recentGroup = recentHeading?.closest<HTMLElement>("[data-sidebar='group']");
    if (!reader || !fileChange || !fileTree || !activeReview || !activeTab || !selectedFile || !recentGroup) {
      throw new Error("Reader 可访问状态证据不完整");
    }
    const rect = reader.getBoundingClientRect();
    const treeRect = fileTree.getBoundingClientRect();
    return {
      reviewId: activeReview.dataset.reviewId,
      fileBefore: fileChange.dataset.before,
      fileAfter: fileChange.dataset.after,
      selectedPath: selectedFile.textContent?.trim(),
      activeView: {
        text: activeTab.textContent?.trim(),
        controls: activeTab.getAttribute("aria-controls"),
      },
      diffLayout: [...reader.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].map((button) => ({
        label: button.getAttribute("aria-label"),
        pressed: button.getAttribute("aria-pressed"),
        disabled: button.disabled,
      })),
      treeWidth: treeRect.width,
      expandedState: [...reader.querySelectorAll<HTMLButtonElement>("button[aria-expanded]")].map((button) => ({
        text: button.textContent?.trim(),
        expanded: button.getAttribute("aria-expanded"),
      })),
      sidebarGroups: [...document.querySelectorAll<HTMLButtonElement>("[data-sidebar='group'] > button[aria-expanded]")]
        .filter((button) => button.offsetParent !== null)
        .map((button) => ({ text: button.textContent?.trim(), expanded: button.getAttribute("aria-expanded") })),
      recentReviewOrder: [...recentGroup.querySelectorAll<HTMLElement>("[data-review-id]")]
        .map((item) => item.dataset.reviewId),
      rect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
      scroll: [...reader.querySelectorAll<HTMLElement>("*")]
        .filter((element) => element.scrollHeight > element.clientHeight)
        .map((element) => ({ aria: element.getAttribute("aria-label"), top: element.scrollTop })),
    };
  });
}

/** 返回 WDIO 构建中 adapter invoke 轨迹的当前 offset。 */
async function invokeTrace() {
  return browser.execute(() => (
    (globalThis as typeof globalThis & { __NARRANT_E2E_TAURI_INVOKES__?: string[] })
      .__NARRANT_E2E_TAURI_INVOKES__ ?? []
  ));
}

/** 清理共享 WDIO DB 中前序 spec 的 Review，确保生命周期证据从精确基线开始。 */
async function isolateReviewLifecycleBaseline(repositoryPath: string) {
  const navigation = await browser.tauri.execute(
    ({ core }) => core.invoke<{ recentReviews: Array<{ review: { reviewId: string } }> }>("load_repository_navigation"),
  );
  for (const recent of navigation.recentReviews) {
    await browser.tauri.execute(
      ({ core }, reviewId) => core.invoke("close_review", { reviewId }),
      recent.review.reviewId,
    );
  }
  await browser.waitUntil(() => {
    const worktrees = linkedWorktrees(repositoryPath);
    return worktrees.length === 1 && worktrees[0] === repositoryPath;
  });
}

describe("进程生命周期 Open Review 真实应用验证", () => {
  it("A→B→A 保留两个 Workspace，Overview/Home 导航不释放，正常退出清理全部", async () => {
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    const tracePath = process.env.NARRANT_E2E_GIT_TRACE!;
    const databasePath = process.env.NARRANT_DATABASE_PATH!;
    await browser.setWindowSize(1400, 900);
    await isolateReviewLifecycleBaseline(repositoryPath);
    await openFixtureRepository(repositoryPath);

    const initialWorktrees = linkedWorktrees(repositoryPath);
    await $("[role='tab']=本地分支").click();
    await selectRef("Base Ref", "refs/heads/retained-base-ref");
    await selectRef("Head Ref", "refs/heads/retained-head-ref");
    await $("button=开始审核").click();
    await expect($("[aria-label='审核阅读器']")).toBeDisplayed();
    const reviewA = await reviewIdForSelection("refs/heads/retained-base-ref", "refs/heads/retained-head-ref");
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
    await selectRef("Base Ref", "refs/heads/retained-head-ref");
    await selectRef("Head Ref", "refs/heads/retained-base-ref");
    await $("button=开始审核").click();
    await expect($("[aria-label='审核阅读器']")).toBeDisplayed();
    const reviewB = await reviewIdForSelection("refs/heads/retained-head-ref", "refs/heads/retained-base-ref");
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

    // 窄窗口下通过真实 Sidebar 完成 B→A→B，且任何 Workspace inspection/lifecycle command 都必须为零。
    await browser.setWindowSize(900, 600);
    const invokesBeforeWarmA = (await invokeTrace()).length;
    const beforeWarmA = await stableGitTraceOffset(tracePath);
    await clickVisibleReview(reviewA);
    await waitForActiveReview(reviewA);
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    const warmAWorkspaceOperations = workspaceOperationInvocations(
      await gitInvocationsSinceStable(tracePath, beforeWarmA),
      [workspaceA, workspaceB],
    );
    if (warmAWorkspaceOperations.length > 0) {
      throw new Error(`warm A 触发 Workspace 操作：${JSON.stringify({
        commands: (await invokeTrace()).slice(invokesBeforeWarmA),
        git: warmAWorkspaceOperations,
      })}`);
    }
    await waitForWorktrees(repositoryPath, afterB);
    expect(linkedWorktrees(repositoryPath)).toContain(workspaceA);
    expect(linkedWorktrees(repositoryPath)).toContain(workspaceB);

    const beforeWarmB = await stableGitTraceOffset(tracePath);
    await clickVisibleReview(reviewB);
    await waitForActiveReview(reviewB);
    expect(workspaceOperationInvocations(await gitInvocationsSinceStable(tracePath, beforeWarmB), [workspaceA, workspaceB])).toEqual([]);
    await waitForWorktrees(repositoryPath, afterB);
    expect(linkedWorktrees(repositoryPath)).toContain(workspaceA);
    expect(linkedWorktrees(repositoryPath)).toContain(workspaceB);

    // Repository Overview 恢复 retained A 不检查 update、不读取 Workspace；允许 Babysit 对固定文件重新确认 eligibility。
    const invokesBeforeOverview = (await invokeTrace()).length;
    await leaveToRepositoryOverview();
    await waitForWorktrees(repositoryPath, afterB);
    await browser.waitUntil(async () => (
      (await invokeTrace()).slice(invokesBeforeOverview).includes("check_branch_review_updates")
    ), { timeoutMsg: "Repository 自身的 Branch Selection 检查没有启动" });
    await stableGitTraceOffset(tracePath);
    const invokesBeforeRepositoryRestore = (await invokeTrace()).length;
    const beforeRepositoryRestore = await stableGitTraceOffset(tracePath);
    await clickVisibleReview(reviewA);
    await waitForActiveReview(reviewA);
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    const repositoryRestoreInvocations = await gitInvocationsSinceStable(tracePath, beforeRepositoryRestore);
    const repositoryRestoreCommands = (await invokeTrace()).slice(invokesBeforeRepositoryRestore);
    expect(repositoryRestoreCommands).toContain("reopen_review");
    expect(repositoryRestoreCommands.filter((command) => (
      command !== "reopen_review" && command !== "check_babysit_file_eligibility"
    ))).toEqual([]);
    expect(repositoryRestoreInvocations.every(({ argv, cwd }) => (
      cwd === repositoryPath
      && argv.includes("cat-file")
      && (argv.includes("-s") || argv.includes("blob"))
    ))).toBe(true);
    expect(workspaceMutationInvocations(repositoryRestoreInvocations)).toEqual([]);
    expect(repositoryRestoreInvocations.filter((invocation) => (
      invocationTouchesWorkspace(invocation, workspaceA)
      || invocationTouchesWorkspace(invocation, workspaceB)
    ))).toEqual([]);
    await waitForWorktrees(repositoryPath, afterB);

    // Home 导航同样不释放 retained Workspace；恢复时只允许 Repository Overview 的通用只读 probe。
    await leaveToRepositoryOverview();
    const brand = await visibleBrandButton();
    await brand.waitForClickable();
    await brand.click();
    await expect($("main [aria-label='最近审核']")).toBeDisplayed();
    await waitForWorktrees(repositoryPath, afterB);

    // 从 Home 恢复允许重新读取 Repository Overview，但不得物化、重置或释放任何 Workspace。
    const invokesBeforeHomeRestore = (await invokeTrace()).length;
    const beforeHomeRestore = await stableGitTraceOffset(tracePath);
    await clickVisibleReview(reviewA);
    await waitForActiveReview(reviewA);
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    const homeRestoreInvocations = await gitInvocationsSinceStable(tracePath, beforeHomeRestore);
    const homeRestoreCommands = (await invokeTrace()).slice(invokesBeforeHomeRestore);
    expect(homeRestoreCommands).toContain("open_repository_overview");
    expect(homeRestoreCommands).toContain("reopen_review");
    expect(homeRestoreCommands).not.toContain("check_branch_review_updates");
    expect(homeRestoreCommands).not.toContain("check_pull_request_review_update");
    expect(homeRestoreInvocations.filter(({ argv }) => argv.some((argument) => (
      argument === "fetch" || argument === "merge-base" || argument === "diff"
    )))).toEqual([]);
    // Home 会重新做 Repository capability probe；允许原仓库 cwd 的只读命令，但禁止任何目标路径访问或生命周期变更。
    expect(workspaceMutationInvocations(homeRestoreInvocations)).toEqual([]);
    expect(homeRestoreInvocations.filter((invocation) => (
      invocationTouchesWorkspace(invocation, workspaceA)
      || invocationTouchesWorkspace(invocation, workspaceB)
    ))).toEqual([]);
    await waitForWorktrees(repositoryPath, afterB);

    // 当前 Presented Review 的重复点击也必须是完整 no-op，不触发 reopen、内容读取或任何 Git invocation。
    const durableBefore = await persistedReview(reviewA);
    const filesBefore = persistenceFingerprint(databasePath);
    const readerBefore = await readerEvidence();
    const invokesBefore = (await invokeTrace()).length;
    const beforeDuplicate = await stableGitTraceOffset(tracePath);
    await clickVisibleReview(reviewA);
    await waitForActiveReview(reviewA);
    await expect($("[aria-label='File Change']")).toBeDisplayed();
    expect(await gitInvocationsSinceStable(tracePath, beforeDuplicate)).toEqual([]);
    await waitForWorktrees(repositoryPath, afterB);
    expect((await invokeTrace()).slice(invokesBefore)).toEqual([]);
    expect(await persistedReview(reviewA)).toEqual(durableBefore);
    expect(persistenceFingerprint(databasePath)).toEqual(filesBefore);
    expect(await readerEvidence()).toEqual(readerBefore);

    // 通过真实 Tauri window close 触发 RunEvent::Exit，再验证正常退出 cleanup。
    await browser.setWindowSize(1200, 800);
    expect(await browser.getWindowSize()).toEqual({ width: 1200, height: 800 });
    Object.defineProperty(browser, "deleteSession", {
      configurable: true,
      value: async () => undefined,
    });
    try {
      await browser.tauri.execute(({ core }) => core.invoke("plugin:window|close", { label: "main" }));
    } catch {
      // window close 会先结束 WebDriver transport，随后才返回 invoke 结果，属于正常退出时序。
    }
    for (let attempt = 0; attempt < 50 && JSON.stringify(linkedWorktrees(repositoryPath)) !== JSON.stringify(initialWorktrees); attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    const finalWorktrees = linkedWorktrees(repositoryPath);
    expect(finalWorktrees).toEqual(initialWorktrees);
    expect(finalWorktrees).not.toContain(workspaceA);
    expect(finalWorktrees).not.toContain(workspaceB);
  });
});

import { navigateHome, visibleBrandButton } from "./review-navigation";

/** 让系统目录选择器在 E2E 中返回 fixture 路径，同时保留真实 Tauri IPC 后端。 */
export async function chooseRepositoryFolder(
  _repositoryPath: string,
  buttonName = "打开仓库文件夹",
) {
  const openButton = await $("main").$(`button=${buttonName}`);
  await openButton.waitForDisplayed();
  await openButton.waitForClickable();
  await openButton.click();
  await browser.waitUntil(async () => (
    await $("[role='dialog']").isDisplayed()
    || await $("[aria-label='Repository']").isDisplayed()
  ), { timeoutMsg: "Rust picker 返回后既没有 Trust dialog，也没有 Repository Overview" });
}

/** 显式切换 E2E 构建的界面语言，不读取执行测试的主机 locale。 */
export async function setE2ELocale(locale: "zh-CN" | "en") {
  await browser.executeAsync((nextLocale, done) => {
    const setter = (window as Window & {
      __NARRANT_E2E_SET_LOCALE__?: (value: string) => Promise<unknown>;
    }).__NARRANT_E2E_SET_LOCALE__;
    if (!setter) {
      done(new Error("E2E locale setter is unavailable"));
      return;
    }
    void setter(nextLocale).then(() => done()).catch(done);
  }, locale);
}

/** 通过当前可见的 desktop/offcanvas sidebar 返回 Home。 */
export async function returnToHome() {
  await navigateHome();
  await browser.waitUntil(async () => (
    await $("button=打开仓库文件夹").isDisplayed()
    || await $("button=Open repository folder").isDisplayed()
  ));
  await browser.keys("Escape");
}

/** 通过仅 WDI0 暴露的函数重新投影持久化 Repository navigation。 */
async function reloadRepositoryNavigation() {
  await browser.executeAsync((done) => {
    const reload = (window as Window & {
      __NARRANT_E2E_RELOAD_REPOSITORY_NAVIGATION__?: () => Promise<void>;
    }).__NARRANT_E2E_RELOAD_REPOSITORY_NAVIGATION__;
    if (!reload) {
      done(new Error("E2E repository navigation reload seam is unavailable"));
      return;
    }
    void reload().then(() => done()).catch(done);
  });
}

/** 点击当前可见且名称精确匹配的 Repository 侧边栏入口。 */
async function openSeededFixture(repositoryPath: string, locale: "zh-CN" | "en", expectReady = true) {
  const name = repositoryPath.split(/[\\/]/).filter(Boolean).at(-1);
  if (!name) throw new Error(`无法从 fixture 路径读取 Repository 名称：${repositoryPath}`);
  const label = locale === "en" ? `Open repository ${name}` : `打开仓库 ${name}`;
  const groupName = locale === "en" ? "Repositories" : "代码仓库";
  let group: WebdriverIO.Element | undefined;
  const findVisibleGroup = async () => {
    for (const candidate of await $$(`button=${groupName}`)) {
      if (await candidate.isDisplayed()) {
        group = candidate;
        return true;
      }
    }
    return false;
  };
  if (!(await findVisibleGroup())) {
    await visibleBrandButton();
  }
  await browser.waitUntil(findVisibleGroup);
  if (!group) throw new Error(`没有可见的 Repository 分组：${groupName}`);
  if (await group.getAttribute("aria-expanded") === "false") {
    await group.waitForClickable();
    await group.click();
  }
  let opened = false;
  await browser.waitUntil(async () => {
    for (const button of await $$(`button[aria-label='${label}']`)) {
      const row = button.$("..");
      if (!(await row.isDisplayed())) continue;
      if (!(await button.isDisplayed()) || !(await button.isClickable())) {
        const repositoryButton = row.$("button");
        if (!(await repositoryButton.isDisplayed()) || !(await repositoryButton.isClickable())) continue;
        // 真实点击 Repository 行会建立 focus-within，并按产品交互展开 Review 列表、显示精确 open action。
        await repositoryButton.click();
      }
      if (await button.isDisplayed() && await button.isClickable()) await button.click();
      else {
        const visiblyPresented = await browser.execute((action) => {
          const style = getComputedStyle(action);
          const rect = action.getBoundingClientRect();
          return style.opacity === "1" && style.pointerEvents !== "none" && rect.width > 0 && rect.height > 0;
        }, button);
        if (!visiblyPresented) continue;
        // embedded WDI0 会被离屏 sidebar layer 干扰 hit-test；仅对实际呈现的精确 action 触发产品 click。
        await browser.execute((action) => action.click(), button);
      }
      opened = true;
      return true;
    }
    return false;
  }, { timeoutMsg: `没有找到可见且可交互的 Repository 入口：${label}` });
  if (!opened) throw new Error(`没有打开 Repository：${label}`);
  if (expectReady) {
    await expect($("[aria-label='Repository']")).toBeDisplayed();
    await expect($("[aria-current='page']")).toHaveText(expect.stringContaining(name));
  }
  await browser.keys("Escape");
}

/** 从真实侧边栏打开已授权 fixture；目录选择与 Trust UI 由 persistence 专项覆盖。 */
export async function openFixtureRepository(repositoryPath: string, locale: "zh-CN" | "en" = "zh-CN") {
  await setE2ELocale(locale);
  await reloadRepositoryNavigation();
  await openSeededFixture(repositoryPath, locale);
}

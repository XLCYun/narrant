/** 让系统目录选择器在 E2E 中返回 fixture 路径，同时保留真实 Tauri IPC 后端。 */
export async function chooseRepositoryFolder(
  repositoryPath: string,
  buttonName = "打开仓库文件夹",
) {
  await browser.execute((path) => {
    (window as Window & { __NARRANT_E2E_DIRECTORY__?: string }).__NARRANT_E2E_DIRECTORY__ = path;
  }, repositoryPath);
  const openButton = await $("main").$(`button=${buttonName}`);
  await openButton.waitForDisplayed();
  await openButton.waitForClickable();
  await openButton.click();
  const result = await browser.executeAsync((done) => {
    const operation = (window as Window & {
      __NARRANT_E2E_REPOSITORY_FOLDER_OPERATION__?: Promise<{ kind: string }>;
    }).__NARRANT_E2E_REPOSITORY_FOLDER_OPERATION__;
    if (!operation) {
      done(new Error("E2E picker operation is unavailable"));
      return;
    }
    void operation.then(done).catch(done);
  });
  if (result.kind !== "trust-required" && result.kind !== "navigated") {
    throw new Error(`系统 picker 导航未完成：${JSON.stringify(result)}`);
  }
  if (result.kind === "trust-required") {
    await browser.waitUntil(async () => await $("[role='dialog']").isDisplayed());
  } else {
    await browser.waitUntil(async () => await $("[aria-label='Repository']").isDisplayed());
  }
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
  const clickVisibleBrand = async () => {
    for (const brand of await $$("button[aria-label='Narrant'], button[aria-label='纳言']")) {
      if (!(await brand.isDisplayed())) continue;
      await brand.waitForClickable();
      await brand.click();
      return true;
    }
    return false;
  };
  if (!(await clickVisibleBrand())) {
    for (const trigger of await $$('[data-slot="sidebar-trigger"]')) {
      if (!(await trigger.isDisplayed())) continue;
      await trigger.waitForClickable();
      await trigger.click();
      break;
    }
    await browser.waitUntil(clickVisibleBrand);
  }
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
  let openedSidebar = false;
  if (!(await findVisibleGroup())) {
    for (const trigger of await $$('[data-slot="sidebar-trigger"]')) {
      if (!(await trigger.isDisplayed())) continue;
      await trigger.waitForClickable();
      await trigger.click();
      openedSidebar = true;
      break;
    }
  }
  await browser.waitUntil(findVisibleGroup);
  if (!group) throw new Error(`没有可见的 Repository 分组：${groupName}`);
  if (await group.getAttribute("aria-expanded") === "false") {
    await group.waitForClickable();
    await group.click();
  }
  await browser.waitUntil(async () => (await $$(`button[aria-label='${label}']`)).length > 0);
  for (const button of await $$(`button[aria-label='${label}']`)) {
    const row = button.$("..");
    await row.waitForDisplayed();
    await row.moveTo();
    // 让精确 action 获得键盘焦点，通过产品自身 focus-within 样式呈现按钮。
    await browser.execute((action) => action.focus(), button);
    await button.waitForDisplayed();
    await button.waitForClickable();
    await button.click();
    if (expectReady) {
      await expect($("[aria-label='Repository']")).toBeDisplayed();
      await expect($("[aria-current='page']")).toHaveText(expect.stringContaining(name));
    }
    if (openedSidebar) {
      // Offcanvas 打开时 trigger 会被遮罩覆盖；Escape 是组件支持的真实关闭交互。
      await browser.keys("Escape");
    }
    return;
  }
  throw new Error(`没有找到精确的 Repository 侧边栏入口：${label}`);
}

/** 从真实侧边栏打开已授权 fixture；目录选择与 Trust UI 由 persistence 专项覆盖。 */
export async function openFixtureRepository(repositoryPath: string, locale: "zh-CN" | "en" = "zh-CN") {
  await setE2ELocale(locale);
  await reloadRepositoryNavigation();
  await openSeededFixture(repositoryPath, locale);
}

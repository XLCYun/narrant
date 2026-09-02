import { navigateHome, visibleBrandButton } from "./review-navigation";
import { clickWebView } from "./webview-interaction";

/** 让系统目录选择器在 E2E 中返回 fixture 路径，同时保留真实 Tauri IPC 后端。 */
export async function chooseRepositoryFolder(
  repositoryPath: string,
  buttonName = "打开仓库文件夹",
) {
  await browser.tauri.execute(
    ({ core }, path) => core.invoke("set_e2e_repository_directory", { path }),
    repositoryPath,
  );
  const openButton = await $("main").$(`button=${buttonName}`);
  await openButton.waitForDisplayed();
  await clickWebView(openButton, "点击打开仓库文件夹失败");
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
  // i18next 的 Promise 先于 React 订阅者提交；等待两帧，保证后续按本地化名称查询稳定 DOM。
  await browser.executeAsync((done) => {
    requestAnimationFrame(() => requestAnimationFrame(() => done()));
  });
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

/** 返回当前可见的 Repository 分组，统一 desktop 与 offcanvas sidebar 查询。 */
async function visibleRepositoryGroup(groupName: string) {
  for (const candidate of await $$(`button=${groupName}`)) {
    if (await candidate.isDisplayed()) return candidate;
  }
}

/** 点击当前可见且名称精确匹配的 Repository 侧边栏入口。 */
async function openSeededFixture(repositoryPath: string, locale: "zh-CN" | "en", expectReady = true) {
  const name = repositoryPath.split(/[\\/]/).filter(Boolean).at(-1);
  if (!name) throw new Error(`无法从 fixture 路径读取 Repository 名称：${repositoryPath}`);
  const label = locale === "en" ? `Open repository ${name}` : `打开仓库 ${name}`;
  const groupName = locale === "en" ? "Repositories" : "代码仓库";
  let group = await visibleRepositoryGroup(groupName);
  if (!group) {
    await visibleBrandButton();
  }
  await browser.waitUntil(async () => {
    group = await visibleRepositoryGroup(groupName);
    return Boolean(group);
  });
  if (!group) throw new Error(`没有可见的 Repository 分组：${groupName}`);
  if (await group.getAttribute("aria-expanded") === "false") {
    await clickWebView(group, "展开 Repository 分组失败");
  }
  let opened = false;
  await browser.waitUntil(async () => {
    for (const button of await $$(`button[aria-label='${label}']`)) {
      const row = await button.$("..");
      if (!(await row.isDisplayed())) continue;
      if (!(await button.isDisplayed())) {
        const repositoryButton = await row.$("button");
        if (!(await repositoryButton.isDisplayed())) continue;
        // 展开真实 Repository 行；展开态会持续呈现精确 open action，兼顾键盘与触屏。
        if (await repositoryButton.getAttribute("aria-expanded") === "false") {
          await clickWebView(repositoryButton, `展开 Repository 行失败：${name}`);
        }
        await browser.waitUntil(async () => await button.isDisplayed(), {
          timeout: 1000,
          timeoutMsg: `Repository action 展开后仍不可见：${label}`,
        });
      }
      if (!(await button.isDisplayed())) continue;
      await button.waitForDisplayed();
      await clickWebView(button, `打开 Repository 失败：${name}`);
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
  await visibleBrandButton();
  const groupName = locale === "en" ? "Repositories" : "代码仓库";
  await browser.waitUntil(async () => {
    if (await visibleRepositoryGroup(groupName)) return true;
    // 首次冷启动时 IPC 可能先于 React projection 提交；串行重载而不制造并发请求。
    await reloadRepositoryNavigation();
    await visibleBrandButton();
    return Boolean(await visibleRepositoryGroup(groupName));
  }, { interval: 1000, timeoutMsg: "Repository navigation 已加载但侧边栏投影尚未提交" });
  await openSeededFixture(repositoryPath, locale);
}

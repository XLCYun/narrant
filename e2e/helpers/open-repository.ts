/** 让系统目录选择器在 E2E 中返回 fixture 路径，同时保留真实 Tauri IPC 后端。 */
export async function chooseRepositoryFolder(
  repositoryPath: string,
  buttonName = "打开仓库文件夹",
) {
  await browser.execute((path) => {
    (window as Window & { __NARRANT_E2E_DIRECTORY__?: string }).__NARRANT_E2E_DIRECTORY__ = path;
  }, repositoryPath);
  const openButton = await $("main").$(`button=${buttonName}`);
  await openButton.waitForClickable();
  await browser.execute((name) => {
    const button = [...document.querySelectorAll("main button")]
      .find((candidate) => candidate.textContent?.trim() === name);
    if (!button) throw new Error(`未找到 E2E 目录按钮：${name}`);
    button.click();
  }, buttonName);
}

/** 将当前桌面视图退回 Home，保留 Review lifecycle 的进程内 runtime。 */
async function returnHome() {
  // 窄窗口可能把桌面侧边栏收进 offcanvas；重新加载前端投影即可回到 Home，Rust lifecycle 不会因此退出。
  const homeReady = await $("main [aria-label='最近审核']").isExisting()
    || await $("main [aria-label='Recent reviews']").isExisting()
    || await $("button=打开仓库文件夹").isExisting()
    || await $("button=Open repository folder").isExisting();
  if (!homeReady) {
    await browser.setWindowSize(1280, 800);
    await browser.execute(() => window.location.reload());
    await browser.waitUntil(async () => (
      await $("button[aria-label='Narrant'], button[aria-label='纳言']").isExisting()
      || await $("button=打开仓库文件夹").isExisting()
      || await $("button=Open repository folder").isExisting()
    ));
  }
}

/** 用真实 Tauri command 准备一个已授权 Repository，避免 E2E 依赖原生系统窗口和不稳定的 Trust Portal。 */
async function seedTrustedRepository(repositoryPath: string) {
  const repository = await browser.tauri.execute(
    ({ core }, path) => core.invoke<{ id: number }>("register_repository", { path }),
    repositoryPath,
  );
  await browser.tauri.execute(
    ({ core }, repositoryId) => core.invoke("grant_repository_workspace_trust_and_open", { repositoryId }),
    repository.id,
  );
  return repository.id;
}

/** 刷新前端投影，再从真实侧边栏进入已授权 Repository Overview。 */
async function openSeededRepository(locale: "zh-CN" | "en") {
  const repositorySection = locale === "en" ? "Repositories" : "代码仓库";
  const repositoryAlreadyInNavigation = await $(
    locale === "en" ? "button[aria-label^='Open repository ']" : "button[aria-label^='打开仓库 ']",
  ).isExisting();
  if (!repositoryAlreadyInNavigation) {
    const recentReview = await $(
      locale === "en" ? "main [aria-label='Recent reviews'] ul button" : "main [aria-label='最近审核'] ul button",
    );
    if (await recentReview.isExisting()) {
      // 旧测试已留下 Review 时，先通过真实 Recent Review 导航恢复 Repository projection，避免二次 reload 丢失 sidebar 列表。
      await recentReview.click();
      await expect($(locale === "en" ? "[aria-label='Review reader']" : "[aria-label='审核阅读器']")).toBeDisplayed();
      await $(locale === "en" ? "button[aria-label='Exit review']" : "button[aria-label='退出审核']").click();
      await expect($(`[aria-label='Repository']`)).toBeDisplayed();
    } else {
      await browser.execute(() => window.location.reload());
    }
  }
  if (await $(`[aria-label='Repository']`).isExisting()) return;
  await browser.waitUntil(async () => await $(`button=${repositorySection}`).isExisting());
  const section = await $(`button=${repositorySection}`);
  if (await section.getAttribute("aria-expanded") !== "true") await section.click();
  let openRepository;
  for (const button of await $$(`button[aria-label]`)) {
    const label = await button.getAttribute("aria-label");
    if (label?.startsWith(locale === "en" ? "Open repository " : "打开仓库 ")) {
      openRepository = button;
      break;
    }
  }
  if (!openRepository) throw new Error("真实侧边栏没有找到已登记 Repository");
  await openRepository.click();
  await expect($(`[aria-label='Repository']`)).toBeDisplayed();
}

/** 显式切换 E2E 构建的界面语言，不读取执行测试的主机 locale。 */
export async function setE2ELocale(locale: "zh-CN" | "en") {
  await browser.execute((nextLocale) => {
    window.localStorage.setItem("narrant.locale", nextLocale);
  }, locale);
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

/** 从首页选择 fixture 仓库，并在首次打开时确认 Workspace Trust。 */
export async function openFixtureRepository(repositoryPath: string, locale: "zh-CN" | "en" = "zh-CN") {
  await setE2ELocale(locale);
  // 从 Review 或 Repository Overview 统一回到 Home；这是展示导航，不会释放 Review Workspace。
  await returnHome();
  await seedTrustedRepository(repositoryPath);
  await openSeededRepository(locale);
}

/** 让系统目录选择器在 E2E 中返回 fixture 路径，同时保留真实 Tauri IPC 后端。 */
export async function chooseRepositoryFolder(
  repositoryPath: string,
  buttonName = "打开仓库文件夹",
) {
  await browser.execute((path) => {
    (window as Window & { __NARRANT_E2E_DIRECTORY__?: string }).__NARRANT_E2E_DIRECTORY__ = path;
  }, repositoryPath);
  const openButton = await $(`button=${buttonName}`);
  await openButton.waitForClickable();
  await openButton.click();
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

/** 从首页选择 fixture 仓库，并在首次打开时确认 Workspace Trust。 */
export async function openFixtureRepository(repositoryPath: string, locale: "zh-CN" | "en" = "zh-CN") {
  await setE2ELocale(locale);
  const closeReview = locale === "en" ? "Close review" : "关闭审核";
  const openFolder = locale === "en" ? "Open repository folder" : "打开仓库文件夹";
  const trustAndOpen = locale === "en" ? "Trust and open" : "信任并打开";
  if (await $(`button=${closeReview}`).isExisting()) {
    const moreActions = locale === "en" ? "More actions" : "更多操作";
    if (await $(`summary=${moreActions}`).isExisting()) await $(`summary=${moreActions}`).click();
    await $(`button=${closeReview}`).click();
  }
  const back = locale === "en" ? "Back to repositories" : "返回仓库列表";
  if (await $(`button=${back}`).isExisting()) {
    await $(`button=${back}`).click();
  }
  await chooseRepositoryFolder(repositoryPath, openFolder);
  await browser.waitUntil(async () => (
    await $(`button=${trustAndOpen}`).isExisting()
    || await $("[aria-label='Repository Entry']").isExisting()
  ));
  if (await $(`button=${trustAndOpen}`).isExisting()) {
    await $(`button=${trustAndOpen}`).click();
  }
  await expect($("[aria-label='Repository Entry']")).toBeDisplayed();
}

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
export async function openFixtureRepository(repositoryPath: string) {
  await setE2ELocale("zh-CN");
  if (await $("button=关闭审核").isExisting()) {
    await $("button=关闭审核").click();
  }
  if (await $("button=返回 Repository Entries").isExisting()) {
    await $("button=返回 Repository Entries").click();
  }
  await chooseRepositoryFolder(repositoryPath);
  await browser.waitUntil(async () => (
    await $("button=信任并打开").isExisting()
    || await $("[aria-label='Repository Entry']").isExisting()
  ));
  if (await $("button=信任并打开").isExisting()) {
    await $("button=信任并打开").click();
  }
  await expect($("[aria-label='Repository Entry']")).toBeDisplayed();
}

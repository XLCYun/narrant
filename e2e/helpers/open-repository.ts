/** 让系统目录选择器在 E2E 中返回 fixture 路径，同时保留真实 Tauri IPC 后端。 */
export async function chooseRepositoryFolder(repositoryPath: string) {
  await browser.execute((path) => {
    (window as Window & { __NARRANT_E2E_DIRECTORY__?: string }).__NARRANT_E2E_DIRECTORY__ = path;
  }, repositoryPath);
  const openButton = await $("button=打开仓库文件夹");
  await openButton.waitForClickable();
  await openButton.click();
}

/** 从首页选择 fixture 仓库，并在首次打开时确认 Workspace Trust。 */
export async function openFixtureRepository(repositoryPath: string) {
  if (await $("button=返回 Repository Entries").isExisting()) {
    await $("button=返回 Repository Entries").click();
  }
  await chooseRepositoryFolder(repositoryPath);
  if (await $("button=信任并打开").isExisting()) {
    await $("button=信任并打开").click();
  }
  await expect($("[aria-label='Repository Entry']")).toBeDisplayed();
}

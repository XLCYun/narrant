import { chooseRepositoryFolder, returnToHome, setE2ELocale } from "./helpers/open-repository";
import { sameFilesystemPath } from "./helpers/worktree-observation";

describe("Repository Persistence", () => {
  it("通过原生目录选择、首次信任并在再次选择时复用仓库", async () => {
    const repositoryPath = process.env.NARRANT_E2E_TRUST_REPOSITORY!;
    await browser.setWindowSize(900, 600);
    await setE2ELocale("zh-CN");
    await returnToHome();

    await chooseRepositoryFolder(repositoryPath);
    const trustDialog = await $("[role='dialog']");
    await expect(trustDialog).toHaveText(expect.stringContaining("hooks、filters 和 Git LFS"));
    const dialogRect = await browser.execute(() => {
      const dialog = document.querySelector<HTMLElement>("[role='dialog']");
      if (!dialog) throw new Error("Trust dialog 没有可测量的可访问节点");
      const rect = dialog.getBoundingClientRect();
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    });
    expect(dialogRect.x).toBeGreaterThanOrEqual(0);
    expect(dialogRect.y).toBeGreaterThanOrEqual(0);
    expect(dialogRect.x + dialogRect.width).toBeLessThanOrEqual(900);
    expect(dialogRect.y + dialogRect.height).toBeLessThanOrEqual(600);
    await $("button=取消").click();
    await expect($("[role='dialog']")).not.toBeDisplayed();
    await expect($("[aria-label='Repository']")).not.toExist();

    await chooseRepositoryFolder(repositoryPath);
    await $("button=信任并打开").click();
    await expect($("[aria-label='Repository']")).toBeDisplayed();
    await returnToHome();

    await chooseRepositoryFolder(repositoryPath);
    await expect($("[role='dialog']")).not.toBeDisplayed();
    await expect($("[aria-label='Repository']")).toBeDisplayed();
    await expect($("body")).not.toHaveText(expect.stringContaining("已信任"));

    const navigation = await browser.tauri.execute(
      ({ core }) => core.invoke<{ repositories: Array<{ canonicalPath: string }> }>("load_repository_navigation"),
    );
    expect(navigation.repositories.filter((repository) => (
      sameFilesystemPath(repository.canonicalPath, repositoryPath)
    ))).toHaveLength(1);

    await returnToHome();
    await setE2ELocale("en");
    await chooseRepositoryFolder(repositoryPath, "Open repository folder");
    await expect($("[aria-label='Repository']")).toBeDisplayed();
    await browser.setWindowSize(1200, 800);
    expect(await browser.getWindowSize()).toEqual({ width: 1200, height: 800 });
  });
});

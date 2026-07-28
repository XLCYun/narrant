import { chooseRepositoryFolder, setE2ELocale } from "./helpers/open-repository";

describe("Repository Catalog 持久化", () => {
  it("通过原生目录选择、首次信任并在再次选择时复用仓库", async () => {
    const repositoryPath = process.env.NARRANT_E2E_TRUST_REPOSITORY!;
    if (await $("button=返回 Repository Entries").isExisting()) {
      await $("button=返回 Repository Entries").click();
    }

    await chooseRepositoryFolder(repositoryPath);
    await expect($("[role='dialog']")).toHaveText(expect.stringContaining("hooks、filters 和 Git LFS"));
    await $("button=取消").click();
    await expect($("[role='dialog']")).not.toExist();
    await expect($("[aria-label='Repository Entry']")).not.toExist();

    await chooseRepositoryFolder(repositoryPath);
    await $("button=信任并打开").click();
    await expect($("[aria-label='Repository Entry']")).toBeDisplayed();
    await $("button=返回 Repository Entries").click();

    await chooseRepositoryFolder(repositoryPath);
    await expect($("[role='dialog']")).not.toExist();
    await expect($("[aria-label='Repository Entry']")).toBeDisplayed();
    await expect($("body")).not.toHaveText(expect.stringContaining("已信任"));

    const entries = await browser.tauri.execute(
      ({ core }) => core.invoke<Array<{ canonicalPath: string }>>("list_repository_entries"),
    );
    expect(entries.filter((entry) => entry.canonicalPath === repositoryPath)).toHaveLength(1);

    await $("button=返回 Repository Entries").click();
    await setE2ELocale("en");
    await chooseRepositoryFolder(repositoryPath, "Open repository folder");
    await expect($("[aria-label='Repository Entry']")).toBeDisplayed();
  });
});

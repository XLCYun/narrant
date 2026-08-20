import { setE2ELocale } from "./helpers/open-repository";

describe("Repository Persistence", () => {
  it("通过真实 Tauri Repository application 首次信任并在再次打开时复用仓库", async () => {
    const repositoryPath = process.env.NARRANT_E2E_TRUST_REPOSITORY!;
    await browser.setWindowSize(1200, 800);
    await setE2ELocale("zh-CN");

    // 原生目录选择器由操作系统拥有，真实 Tauri command 作为稳定 seed 保留同一业务边界；
    // Trust-required、确认授权、再次打开与 canonical 去重均在真实应用进程中验证。
    const registered = await browser.tauri.execute(
      ({ core }, path) => core.invoke<{ id: number; canonicalPath: string }>("register_repository", { path }),
      repositoryPath,
    );
    expect(registered.canonicalPath).toBe(repositoryPath);

    const beforeTrust = await browser.tauri.execute(
      ({ core }, repositoryId) => core.invoke<{ kind: string; repository: { canonicalPath: string } }>("open_repository_overview", { repositoryId }),
      registered.id,
    );
    expect(beforeTrust.kind).toBe("trust-required");
    expect(beforeTrust.repository.canonicalPath).toBe(repositoryPath);

    const overview = await browser.tauri.execute(
      ({ core }, repositoryId) => core.invoke<{ repository: { id: number; canonicalPath: string } }>("grant_repository_workspace_trust_and_open", { repositoryId }),
      registered.id,
    );
    expect(overview.repository.id).toBe(registered.id);
    expect(overview.repository.canonicalPath).toBe(repositoryPath);

    const afterTrust = await browser.tauri.execute(
      ({ core }, repositoryId) => core.invoke<{ kind: string; overview?: { repository: { id: number } } }>("open_repository_overview", { repositoryId }),
      registered.id,
    );
    expect(afterTrust.kind).toBe("ready");
    expect(afterTrust.overview?.repository.id).toBe(registered.id);

    const navigation = await browser.tauri.execute(
      ({ core }) => core.invoke<{ repositories: Array<{ id: number; canonicalPath: string }> }>("load_repository_navigation"),
    );
    const matchingRepositories = navigation.repositories.filter((repository) => repository.canonicalPath === repositoryPath);
    expect(matchingRepositories).toHaveLength(1);
    expect(matchingRepositories[0].id).toBe(registered.id);
  });
});

import { tauriTransport, type TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type {
  OpenRepositoryResult,
  RepositoryApplicationAdapter,
  RepositoryEntry,
} from "@/repository/repository-application";

/** 创建以 Repository Entry 与 Workspace Trust 为 seam 的 Tauri adapter。 */
export function createTauriRepositoryApplicationAdapter(
  transport: TypedTauriTransport = tauriTransport,
): RepositoryApplicationAdapter {
  async function loadContext(repository: RepositoryEntry) {
    const repositoryEntryId = repository.id;
    const { savedReviews, reviews } = await loadReviews(repositoryEntryId);
    return { repository, savedReviews, reviews };
  }

  async function loadReviews(repositoryEntryId: number) {
    const [savedReviews, reviews] = await Promise.all([
      transport.invoke("list_saved_reviews", { repositoryEntryId }),
      transport.invoke("list_reviews", { repositoryEntryId }),
    ]);
    return { savedReviews, reviews };
  }

  return {
    async loadCatalog() {
      const [repositories, recentReviews] = await Promise.all([
        transport.invoke("list_repository_entries", {}),
        transport.invoke("list_reviews", { repositoryEntryId: null }),
      ]);
      return { repositories, recentReviews };
    },

    register(path) {
      return transport.invoke("register_repository_entry", { path });
    },

    async open(repositoryEntryId): Promise<OpenRepositoryResult> {
      const repository = await transport.invoke("open_repository_entry", { id: repositoryEntryId });
      if (!repository.available) return { kind: "unavailable", repository };
      const trusted = await transport.invoke("repository_workspace_trust", { repositoryEntryId });
      if (!trusted) return { kind: "trust-required", repository };
      return { kind: "ready", context: await loadContext(repository) };
    },

    async grantTrust(repository) {
      await transport.invoke("grant_repository_workspace_trust", { repositoryEntryId: repository.id });
      return loadContext(repository);
    },

    loadReviews,

    loadBranches(repositoryEntryId, signal) {
      return transport.invokeCancellable("list_local_refs", { repositoryEntryId }, signal);
    },

    markOpened(repositoryEntryId) {
      return transport.invoke("open_repository_entry", { id: repositoryEntryId });
    },
  };
}

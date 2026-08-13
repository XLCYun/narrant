import { tauriTransport, type TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type {
  OpenRepositoryResult,
  RepositoryApplicationAdapter,
} from "@/repository/repository-application";

/** 创建以 Repository Entry 与 Workspace Trust 为 seam 的 Tauri adapter。 */
export function createTauriRepositoryApplicationAdapter(
  transport: TypedTauriTransport = tauriTransport,
): RepositoryApplicationAdapter {
  return {
    async loadCatalog() {
      return transport.invoke("load_repository_catalog", {});
    },

    register(path) {
      return transport.invoke("register_repository_entry", { path });
    },

    async open(repositoryEntryId): Promise<OpenRepositoryResult> {
      return transport.invoke("open_repository_overview", { repositoryEntryId });
    },

    async grantTrust(repository) {
      return transport.invoke("grant_repository_workspace_trust_and_open", {
        repositoryEntryId: repository.id,
      });
    },

    loadReviews(repositoryEntryId) {
      return transport.invoke("load_repository_reviews", { repositoryEntryId });
    },

    loadBranches(repositoryEntryId, signal) {
      return transport.invokeCancellable("list_local_refs", { repositoryEntryId }, signal);
    },

  };
}

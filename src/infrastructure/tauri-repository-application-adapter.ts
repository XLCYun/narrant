import { tauriTransport, type TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type {
  OpenRepositoryResult,
  RepositoryApplicationAdapter,
} from "@/repository/repository-application";

/** 创建以 Repository 与 Workspace Trust 为 seam 的 Tauri adapter。 */
export function createTauriRepositoryApplicationAdapter(
  transport: TypedTauriTransport = tauriTransport,
): RepositoryApplicationAdapter {
  return {
    async loadNavigation() {
      return transport.invoke("load_repository_navigation", {});
    },

    register(path) {
      return transport.invoke("register_repository", { path });
    },

    async open(repositoryId): Promise<OpenRepositoryResult> {
      return transport.invoke("open_repository_overview", { repositoryId });
    },

    async grantTrust(repository) {
      return transport.invoke("grant_repository_workspace_trust_and_open", {
        repositoryId: repository.id,
      });
    },

    loadReviews(repositoryId) {
      return transport.invoke("load_repository_reviews", { repositoryId });
    },

    loadBranches(repositoryId, signal) {
      return transport.invokeCancellable("list_local_refs", { repositoryId }, signal);
    },

  };
}

import { tauriTransport, type TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type { PullRequestDiscoveryApplicationAdapter } from "@/github/pull-request-discovery-application";

export function createTauriPullRequestDiscoveryApplicationAdapter(
  transport: TypedTauriTransport = tauriTransport,
): PullRequestDiscoveryApplicationAdapter {
  return {
    /** 列出 Repository Entry 全部 GitHub remotes 对应的 Pull Requests。 */
    discover(repositoryEntryId: number) {
      return transport.invoke("discover_github_pull_requests", { repositoryEntryId });
    },
  };
}

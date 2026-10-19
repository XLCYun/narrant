import { tauriTransport, type TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type { PullRequestDiscoveryApplicationAdapter } from "@/github/pull-request-discovery-application";

/** 将有界的 Discovery 查询交给类型化 Tauri 传输层。 */
export function createTauriPullRequestDiscoveryApplicationAdapter(
  transport: TypedTauriTransport = tauriTransport,
): PullRequestDiscoveryApplicationAdapter {
  return {
    /** 每个选定来源只读取一页 Pull Request metadata。 */
    discover(repositoryId, query) {
      return transport.invoke("discover_github_pull_requests", { repositoryId, query });
    },
  };
}

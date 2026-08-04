import { tauriTransport, type TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type {
  PullRequestDiscoveryApplicationAdapter,
  PullRequestSelection,
} from "@/github/pull-request-discovery-application";

export function createTauriPullRequestDiscoveryApplicationAdapter(
  transport: TypedTauriTransport = tauriTransport,
): PullRequestDiscoveryApplicationAdapter {
  async function discover(repositoryEntryId: number) {
    return transport.invoke("discover_github_pull_requests", { repositoryEntryId });
  }

  return {
    discover,

    async resolve(selection: PullRequestSelection) {
      const groups = await discover(selection.repositoryEntryId);
      const group = groups.find((candidate) =>
        candidate.repository.owner.toLowerCase() === selection.owner.toLowerCase()
        && candidate.repository.name.toLowerCase() === selection.repository.toLowerCase());
      if (group?.error) throw new Error(group.error);
      const pullRequest = group?.pullRequests.find((candidate) => candidate.number === selection.number);
      if (!group || !pullRequest) throw new Error("当前无法找到这个 Pull Request。");
      return { repository: group.repository, pullRequest };
    },
  };
}

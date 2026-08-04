import type { DiscoveredPullRequest, PullRequestGroup } from "@/lib/review-types";

export type PullRequestSelection = {
  repositoryEntryId: number;
  owner: string;
  repository: string;
  number: number;
};

export type PullRequestContext = {
  repository: PullRequestGroup["repository"];
  pullRequest: DiscoveredPullRequest;
};

/** Repository Entry 的 GitHub Pull Request discovery interface。 */
export interface PullRequestDiscoveryApplicationAdapter {
  discover(repositoryEntryId: number): Promise<PullRequestGroup[]>;
  resolve(selection: PullRequestSelection): Promise<PullRequestContext>;
}

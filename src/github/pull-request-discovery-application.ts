import type { PullRequestDiscoveryQuery, PullRequestGroup } from "@/lib/review-types";

/** Repository 的 GitHub Pull Request discovery interface。 */
export interface PullRequestDiscoveryApplicationAdapter {
  discover(repositoryId: number, query: PullRequestDiscoveryQuery): Promise<PullRequestGroup[]>;
}

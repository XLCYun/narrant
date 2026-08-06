import type { PullRequestGroup } from "@/lib/review-types";

/** Repository Entry 的 GitHub Pull Request discovery interface。 */
export interface PullRequestDiscoveryApplicationAdapter {
  discover(repositoryEntryId: number): Promise<PullRequestGroup[]>;
}

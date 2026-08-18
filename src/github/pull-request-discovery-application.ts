import type { PullRequestGroup } from "@/lib/review-types";

/** Repository 的 GitHub Pull Request discovery interface。 */
export interface PullRequestDiscoveryApplicationAdapter {
  discover(repositoryId: number): Promise<PullRequestGroup[]>;
}

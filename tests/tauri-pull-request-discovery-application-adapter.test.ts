import { describe, expect, it, vi } from "vitest";

import { createTauriPullRequestDiscoveryApplicationAdapter } from "@/infrastructure/tauri-pull-request-discovery-application-adapter";
import type { TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type { PullRequestGroup } from "@/lib/review-types";

const groups: PullRequestGroup[] = [{
  repository: { owner: "Octo-Org", name: "Octo-Repo" },
  remoteNames: ["origin"],
  error: null,
  pullRequests: [{
    number: 42,
    title: "Typed discovery",
    description: "",
    author: "octocat",
    state: "open",
    draft: false,
    url: "https://github.com/octo-org/octo-repo/pull/42",
    updatedAt: "2026-08-04T00:00:00Z",
    base: { ref: "main", sha: "base" },
    head: { ref: "feature", sha: "head" },
  }],
}];

describe("Tauri Pull Request Discovery application adapter", () => {
  it("按 Selection identity 大小写不敏感地解析 Pull Request context", async () => {
    const invoke = vi.fn().mockResolvedValue(groups);
    const adapter = createTauriPullRequestDiscoveryApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await expect(adapter.resolve({
      repositoryEntryId: 17,
      owner: "octo-org",
      repository: "octo-repo",
      number: 42,
    })).resolves.toEqual({
      repository: groups[0].repository,
      pullRequest: groups[0].pullRequests[0],
    });
    expect(invoke).toHaveBeenCalledWith("discover_github_pull_requests", { repositoryEntryId: 17 });
  });
});

import { tauriTransport, type TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type { GitHubOAuthApplicationAdapter } from "@/github/github-oauth-application";

export function createTauriGitHubOAuthApplicationAdapter(
  transport: TypedTauriTransport = tauriTransport,
): GitHubOAuthApplicationAdapter {
  return {
    loadStatus() {
      return transport.invoke("github_auth_status", {});
    },

    refreshIdentity() {
      return transport.invoke("refresh_github_identity", {});
    },

    beginAuthorization() {
      return transport.invoke("start_github_authorization", {});
    },

    async completeAuthorization(authorizationId) {
      await transport.invoke("complete_github_authorization", { authorizationId });
      return transport.invoke("github_auth_status", {});
    },

    async signOut() {
      try {
        await transport.invoke("sign_out_github", {});
        const status = await transport.invoke("github_auth_status", {});
        return { kind: "signed-out", status } as const;
      } catch (reason) {
        try {
          const status = await transport.invoke("github_auth_status", {});
          return { kind: "failed", detail: String(reason), status } as const;
        } catch {
          return { kind: "failed", detail: String(reason) } as const;
        }
      }
    },
  };
}

import type { GitHubIdentity } from "@/components/github-identity-entry";

export type GitHubAuthStatus = {
  configured: boolean;
  signedIn: boolean;
  appInstallUrl: string | null;
  identity: GitHubIdentity | null;
};

export type DeviceAuthorization = {
  authorizationId: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
};

/** GitHub App OAuth 与当前 GitHub Identity 的应用 interface。 */
export interface GitHubOAuthApplicationAdapter {
  loadStatus(): Promise<GitHubAuthStatus>;
  refreshIdentity(): Promise<GitHubIdentity>;
  beginAuthorization(): Promise<DeviceAuthorization>;
  completeAuthorization(authorizationId: string): Promise<GitHubAuthStatus>;
  signOut(): Promise<
    | { kind: "signed-out"; status: GitHubAuthStatus }
    | { kind: "failed"; detail: string; status?: GitHubAuthStatus }
  >;
}

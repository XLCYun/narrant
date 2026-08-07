import { useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { GitHubAuthorizationControls } from "@/components/github-authorization-controls";
import type { GitHubAuthStatus, DeviceAuthorization } from "@/github/github-oauth-application";
import type { GitHubIdentityLifecycleBinding } from "@/github/use-github-identity-lifecycle";
import { i18n } from "@/lib/i18n";

const openUrl = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));
const writeText = vi.hoisted(() => vi.fn());

const authorization: DeviceAuthorization = {
  authorizationId: "authorization-1",
  userCode: "ABCD-EFGH",
  verificationUri: "https://github.com/login/device",
  expiresIn: 900,
  interval: 5,
};

const signedOut: GitHubAuthStatus = {
  configured: true,
  signedIn: false,
  appInstallUrl: "https://github.com/apps/narrant/installations/new",
  identity: null,
};

/** 使用本地状态驱动授权展示，不跨越 lifecycle interface。 */
function AuthorizationControlsHarness() {
  const [status, setStatus] = useState(signedOut);
  const [pending, setPending] = useState<DeviceAuthorization>();
  const identity: GitHubIdentityLifecycleBinding = {
    status,
    authorization: pending,
    transition: { kind: "stable" },
    beginAuthorization: async () => {
      setPending(authorization);
      return { kind: "started", authorization };
    },
    completeAuthorization: async () => {
      setPending(undefined);
      setStatus({
        ...signedOut,
        signedIn: true,
        identity: { id: 42, login: "octocat", name: null, avatarPath: null },
      });
      return { kind: "authorized" };
    },
    signOut: async () => ({ kind: "signed-out" }),
    retry: async () => undefined,
  };
  return <GitHubAuthorizationControls identity={identity} onConnect={() => undefined} />;
}

afterEach(() => {
  cleanup();
  openUrl.mockReset();
  writeText.mockReset();
});

beforeEach(async () => {
  await i18n.changeLanguage("zh-CN");
  vi.stubGlobal("navigator", { clipboard: { writeText } });
});

describe("GitHub authorization controls", () => {
  it("通过 lifecycle interface 呈现 Device Flow、复制反馈和授权完成状态", async () => {
    render(<AuthorizationControlsHarness />);

    expect(screen.getByText(/组织仓库可能需要管理员审批/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: signedOut.appInstallUrl! }));
    expect(openUrl).toHaveBeenCalledWith(signedOut.appInstallUrl);

    fireEvent.click(screen.getByRole("button", { name: "我已安装，继续" }));
    expect(await screen.findByLabelText("设备码 ABCD-EFGH")).toBeInTheDocument();
    expect(openUrl).toHaveBeenCalledWith(authorization.verificationUri);

    fireEvent.click(screen.getByRole("button", { name: "复制设备码" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(authorization.userCode));
    expect(screen.getByRole("button", { name: "设备码已复制" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "我已在 GitHub 完成授权" }));
    expect(await screen.findByRole("button", { name: "退出 GitHub" })).toBeInTheDocument();
  });
});

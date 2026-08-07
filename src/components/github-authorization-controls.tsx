import { useState } from "react";
import { Copy01Icon } from "@hugeicons/core-free-icons";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import type { GitHubIdentityLifecycleBinding } from "@/github/use-github-identity-lifecycle";

/** GitHub 授权展示所需的 lifecycle interface 与页面回调。 */
type GitHubAuthorizationControlsProps = {
  identity: GitHubIdentityLifecycleBinding;
  onConnect: () => void;
  onAuthorized?: () => void;
};

/** 呈现 GitHub App 安装、Device Flow、错误恢复和退出操作。 */
export function GitHubAuthorizationControls({
  identity,
  onConnect,
  onAuthorized,
}: GitHubAuthorizationControlsProps) {
  const { t } = useTranslation(["common", "github", "review"]);
  const [copiedValue, setCopiedValue] = useState<"device-code" | "authorization-url">();
  const status = identity.status;
  const authorization = identity.authorization;
  const authorizing = identity.transition.kind === "starting-authorization"
    || identity.transition.kind === "completing-authorization";

  /** 将用户主动选择的授权信息复制到系统剪贴板。 */
  async function copyAuthorizationValue(value: string, copied: "device-code" | "authorization-url") {
    try {
      await navigator.clipboard.writeText(value);
      setCopiedValue(copied);
    } catch (reason) {
      toast.error(t("common:transientError"), { description: String(reason) });
    }
  }

  /** 开始 Device Flow，并由展示代码打开 verification URI。 */
  async function startAuthorization() {
    setCopiedValue(undefined);
    const result = await identity.beginAuthorization();
    if (result.kind === "started") await openUrl(result.authorization.verificationUri);
  }

  /** 完成授权，并在 lifecycle 提交 GitHub Identity 后通知页面。 */
  async function completeAuthorization() {
    const result = await identity.completeAuthorization();
    if (result.kind === "authorized") onAuthorized?.();
  }

  return <div className="min-w-0 space-y-3 font-mono text-sm">
    {!status && <Button variant="ghost" className="w-full justify-start" onClick={onConnect}>{t("github:connect")}</Button>}
    {status?.signedIn && <Button variant="ghost" className="w-full justify-start" onClick={() => void identity.signOut()}>{t("github:signOut")}</Button>}
    {status && !status.signedIn && (!status.configured
      ? <p className="px-2 text-xs text-muted-foreground">{t("github:unconfigured")}</p>
      : authorization
        ? <div className="min-w-0 space-y-5 py-2">
          <p className="text-center leading-6">
            {t("github:authorizationPromptBefore")} {" "}
            <span>
              <a
                className="break-all underline underline-offset-4 hover:text-foreground"
                href={authorization.verificationUri}
                onClick={(event) => {
                  event.preventDefault();
                  void openUrl(authorization.verificationUri);
                }}
              >
                {authorization.verificationUri}
              </a>
              <Button
                variant="ghost"
                size="icon-xs"
                className="ml-1 inline-flex cursor-pointer align-middle"
                aria-label={copiedValue === "authorization-url" ? t("github:linkCopied") : t("github:copyLink")}
                onClick={() => void copyAuthorizationValue(authorization.verificationUri, "authorization-url")}
              >
                <Icon icon={Copy01Icon} />
              </Button>
            </span>{" "}
            {t("github:authorizationPromptAfter")}
          </p>
          <div
            aria-label={`${t("github:deviceCode")} ${authorization.userCode}`}
            className="grid w-full grid-cols-[repeat(4,minmax(0,1fr))_auto_repeat(4,minmax(0,1fr))] items-center gap-1.5 sm:gap-2"
          >
            {Array.from(authorization.userCode).map((character, index) => (
              character === "-"
                ? <span key={`${character}-${index}`} aria-hidden className="px-0.5 text-center text-2xl font-bold">-</span>
                : <span
                  key={`${character}-${index}`}
                  aria-hidden
                  className="flex aspect-square min-w-0 items-center justify-center rounded-md border bg-input/20 text-xl font-bold"
                >
                  {character}
                </span>
            ))}
          </div>
          <Button variant="outline" className="w-full" onClick={() => void copyAuthorizationValue(authorization.userCode, "device-code")}>
            {copiedValue === "device-code" ? t("github:deviceCodeCopied") : t("github:copyDeviceCode")}
          </Button>
          <Button className="w-full" disabled={authorizing} onClick={() => void completeAuthorization()}>{t("github:authorizationComplete")}</Button>
        </div>
        : <div className="space-y-6">
          {status.appInstallUrl && <div className="space-y-3 text-center">
            <p className="leading-6">
              <a
                className="break-all text-foreground underline underline-offset-4"
                href={status.appInstallUrl}
                onClick={(event) => {
                  event.preventDefault();
                  void openUrl(status.appInstallUrl!);
                }}
              >
                {status.appInstallUrl}
              </a>
              <Button
                variant="ghost"
                size="icon-xs"
                className="ml-1 inline-flex cursor-pointer align-middle"
                aria-label={copiedValue === "authorization-url" ? t("github:linkCopied") : t("github:copyLink")}
                onClick={() => void copyAuthorizationValue(status.appInstallUrl!, "authorization-url")}
              >
                <Icon icon={Copy01Icon} />
              </Button>
            </p>
            <p className="leading-6 text-muted-foreground">{t("github:installHelp")}</p>
          </div>}
          <Button className="w-full" disabled={authorizing} onClick={() => void startAuthorization()}>{t("github:installedContinue")}</Button>
        </div>)}
    {identity.error && <div className="space-y-2 px-2">
      <AuthorizationErrorMessage message={identity.error.detail} />
      {identity.error.operation === "load-status" && <Button variant="outline" onClick={() => void identity.retry()}>{t("common:retry")}</Button>}
    </div>}
  </div>;
}

/** 保留 GitHub 错误中的可操作链接，同时避免让 lifecycle 了解展示规则。 */
function AuthorizationErrorMessage({ message }: { message: string }) {
  const { t } = useTranslation("review");
  const match = message.match(/(https:\/\/github\.com\/[^\s]+)/);
  const text = match ? message.replace(match[1], "") : message;
  return <p role="alert">
    {text}
    {match && <> <a href={match[1]} onClick={(event) => { event.preventDefault(); void openUrl(match[1]); }}>{t("viewOnGitHub")}</a></>}
  </p>;
}

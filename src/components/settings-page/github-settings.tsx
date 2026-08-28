import { useState } from "react";
import { GithubIcon } from "@hugeicons/core-free-icons";

import { githubIdentityAvatarUrl } from "@/components/github-identity-entry";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Icon } from "@/components/ui/icon";
import type { GitHubIdentityLifecycleBinding } from "@/github/use-github-identity-lifecycle";

/** GitHub Identity 设置拥有退出确认、失败恢复和头像投影。 */
export function GitHubSettings({ identity, initiallyConfirming = false }: { identity: GitHubIdentityLifecycleBinding; initiallyConfirming?: boolean }) {
  const [confirming, setConfirming] = useState(initiallyConfirming);
  const profile = identity.status?.identity;
  const avatarUrl = githubIdentityAvatarUrl(profile?.avatarPath);
  const signingOut = identity.transition.kind === "signing-out";
  const displayName = profile?.name?.trim() || profile?.login || "已连接 GitHub";

  /** 确认后调用共享 lifecycle；失败时保留弹层外身份并允许再次提交。 */
  async function confirmSignOut() {
    const result = await identity.signOut();
    if (result.kind === "signed-out") setConfirming(false);
  }

  return <section aria-labelledby="github-settings-title">
    <div className="flex items-center gap-3"><Icon icon={GithubIcon} className="size-5" /><h2 id="github-settings-title" className="font-heading text-xl font-semibold">GitHub</h2></div>
    <p className="mt-1 text-sm text-muted-foreground">管理此设备当前授权的 GitHub Identity。</p>
    {identity.status?.signedIn && <div className="mt-6 flex flex-wrap items-center gap-4 rounded-xl border p-4">
      {profile ? <Avatar aria-label={`${displayName} 的 GitHub 头像`} className="size-12">{avatarUrl && <AvatarImage src={avatarUrl} alt="" />}<AvatarFallback>{displayName.slice(0, 2).toUpperCase()}</AvatarFallback></Avatar> : <Icon icon={GithubIcon} className="size-10" />}
      <div className="min-w-0 flex-1"><strong className="block truncate">{displayName}</strong>{profile && <span className="block truncate text-sm text-muted-foreground">@{profile.login}</span>}</div>
      <Button variant="destructive" onClick={() => setConfirming(true)}>退出 GitHub</Button>
    </div>}
    {identity.error?.operation === "sign-out" && <div className="mt-4 space-y-3 rounded-lg border border-destructive/30 bg-destructive/5 p-3"><p role="alert" className="text-sm text-destructive">退出失败：{identity.error.detail}</p><Button variant="outline" disabled={signingOut} onClick={() => void identity.signOut()}>{signingOut ? "正在退出…" : "重试退出"}</Button></div>}
    <Dialog open={confirming} onOpenChange={setConfirming}>
      <DialogContent>
        <DialogHeader><DialogTitle>确认退出 GitHub？</DialogTitle><DialogDescription>退出会删除此设备上的 GitHub 授权凭据以及身份和头像缓存。Repository、Review 与本地代码不会被删除。</DialogDescription></DialogHeader>
        <DialogFooter><DialogClose render={<Button variant="outline" />} disabled={signingOut}>取消</DialogClose><Button variant="destructive" disabled={signingOut} onClick={() => void confirmSignOut()}>{signingOut ? "正在退出…" : "确认退出"}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  </section>;
}

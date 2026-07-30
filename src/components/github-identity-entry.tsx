import { RiGithubFill, RiLogoutBoxRLine } from "@remixicon/react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Button } from "@/components/ui/button";

/** WebView 可以安全呈现的 GitHub 公开身份。 */
export type GitHubIdentity = {
  id: number;
  login: string;
  name: string | null;
  avatarPath: string | null;
};

type Props = {
  signedIn: boolean;
  identity?: GitHubIdentity | null;
  onConnect: () => void;
  onSignOut: () => void;
  className?: string;
};

/** 从显示名称或 login 生成最多两个字符的头像占位文本。 */
function identityInitials(identity: GitHubIdentity) {
  const source = identity.name?.trim() || identity.login;
  const words = source.split(/\s+/).filter(Boolean);
  return words.length > 1
    ? words.slice(0, 2).map((word) => Array.from(word)[0]).join("").toUpperCase()
    : Array.from(source).slice(0, 2).join("").toUpperCase();
}

/** 全局 GitHub Identity 入口，统一承载连接、身份确认和退出。 */
export function GitHubIdentityEntry({ signedIn, identity, onConnect, onSignOut, className }: Props) {
  const { t } = useTranslation("github");

  if (!signedIn) {
    return <Button variant="ghost" className={className} onClick={onConnect}>
      <RiGithubFill />
      <span className="truncate">{t("connect")}</span>
    </Button>;
  }

  const compactName = identity?.name?.trim() || identity?.login || t("connected");
  const avatarUrl = identity?.avatarPath
    ? ("__TAURI_INTERNALS__" in window ? convertFileSrc(identity.avatarPath) : identity.avatarPath)
    : null;
  return <DropdownMenu>
    <DropdownMenuTrigger
      aria-label={compactName}
      className={className}
      render={<Button variant="ghost" />}
    >
      {identity
        ? <Avatar className="size-8">
          {avatarUrl && <AvatarImage src={avatarUrl} alt="" />}
          <AvatarFallback>{identityInitials(identity)}</AvatarFallback>
        </Avatar>
        : <RiGithubFill />}
      <span className="truncate">{compactName}</span>
    </DropdownMenuTrigger>
    <DropdownMenuContent side="top" align="start" className="w-64">
      {identity && <>
        <DropdownMenuGroup>
          <DropdownMenuLabel className="flex items-center gap-3">
            <Avatar className="size-9">
              {avatarUrl && <AvatarImage src={avatarUrl} alt="" />}
              <AvatarFallback>{identityInitials(identity)}</AvatarFallback>
            </Avatar>
            <span className="min-w-0">
              <span className="block truncate font-medium">{compactName}</span>
              <span className="block truncate text-xs font-normal text-muted-foreground">@{identity.login}</span>
            </span>
          </DropdownMenuLabel>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
      </>}
      <DropdownMenuItem variant="destructive" onClick={onSignOut}>
        <RiLogoutBoxRLine />
        {t("signOut")}
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>;
}

import { LogoutSquare01Icon } from "@hugeicons/core-free-icons";
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
import { GitHubMark } from "@/components/brand/github-mark";
import { Icon } from "@/components/ui/icon";

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
  error?: string;
  onConnect: () => void;
  onSignOut: () => void;
  onRetry?: () => void;
  className?: string;
};

/** 将缓存头像路径转换为当前 WebView 可以加载的资源地址。 */
export function githubIdentityAvatarUrl(avatarPath: string | null | undefined) {
  if (!avatarPath) return null;
  return "__TAURI_INTERNALS__" in window ? convertFileSrc(avatarPath) : avatarPath;
}

/** 从显示名称或 login 生成最多两个字符的头像占位文本。 */
function identityInitials(identity: GitHubIdentity) {
  const source = identity.name?.trim() || identity.login;
  const words = source.split(/\s+/).filter(Boolean);
  return words.length > 1
    ? words.slice(0, 2).map((word) => Array.from(word)[0]).join("").toUpperCase()
    : Array.from(source).slice(0, 2).join("").toUpperCase();
}

/** 全局 GitHub Identity 入口，统一承载连接、身份确认和退出。 */
export function GitHubIdentityEntry({ signedIn, identity, error, onConnect, onSignOut, onRetry, className }: Props) {
  const { t } = useTranslation(["github", "common"]);

  if (!signedIn) {
    return <Button variant="ghost" className={className} onClick={error && onRetry ? onRetry : onConnect}>
      <GitHubMark className="size-[22px] shrink-0" />
      <span className="truncate">{error ? t("retryStatus") : t("connect")}</span>
    </Button>;
  }

  const compactName = identity?.name?.trim() || identity?.login || t("connected");
  const avatarUrl = githubIdentityAvatarUrl(identity?.avatarPath);
  return <div className="min-w-0 space-y-1">
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={compactName}
        className={className}
        render={<Button variant="ghost" />}
      >
        {identity
          ? <Avatar className="size-9">
            {avatarUrl && <AvatarImage src={avatarUrl} alt="" />}
            <AvatarFallback>{identityInitials(identity)}</AvatarFallback>
          </Avatar>
          : <GitHubMark className="size-[22px] shrink-0" />}
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
          <Icon icon={LogoutSquare01Icon} />
          {t("signOut")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
    {error && <div className="space-y-1 px-2 text-xs text-destructive">
      <p role="alert" className="break-words">{error}</p>
      {onRetry && <Button variant="outline" size="sm" className="w-full" onClick={onRetry}>{t("retryAction")}</Button>}
    </div>}
  </div>;
}

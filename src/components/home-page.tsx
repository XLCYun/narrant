import { useEffect, useState, type CSSProperties, type MouseEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { RiFolderOpenLine } from "@remixicon/react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Sidebar, SidebarContent, SidebarGroup, SidebarGroupContent, SidebarGroupLabel, SidebarMenu, SidebarMenuButton, SidebarMenuItem, SidebarProvider, SidebarTrigger } from "@/components/ui/sidebar";

type Review = { reviewId: string; repositoryEntryId: number; repositoryPath: string; baseRef: string; headRef: string; lastOpenedAt: number };
type Props = { reviews: Review[]; error?: string; loading?: boolean; onOpenReview: (review: Review) => void; onOpenFolder: (path: string) => void };
const preferenceKey = "narrant.sidebar-preferences";

/** 最近审核首页以及仅保存于浏览器的主侧边栏偏好。 */
export function HomePage({ reviews, error, loading = false, onOpenReview, onOpenFolder }: Props) {
  const { t } = useTranslation();
  const [preferences, setPreferences] = useState(() => JSON.parse(window.localStorage?.getItem(preferenceKey) ?? `{"open":${window.innerWidth >= 1200},"width":280}`));
  const openSidebar = preferences.open !== false;
  const sidebarWidth = typeof preferences.width === "number" ? preferences.width : 280;
  useEffect(() => { window.localStorage?.setItem(preferenceKey, JSON.stringify({ open: openSidebar, width: sidebarWidth })); }, [openSidebar, sidebarWidth]);
  function resizeSidebar(event: MouseEvent<HTMLDivElement>) { const startX = event.clientX; const startWidth = sidebarWidth; const resize = (move: globalThis.MouseEvent) => setPreferences((value: { open?: boolean; width?: number }) => ({ ...value, width: Math.min(480, Math.max(220, startWidth + move.clientX - startX)) })); const stop = () => { document.removeEventListener("mousemove", resize); document.removeEventListener("mouseup", stop); }; document.addEventListener("mousemove", resize); document.addEventListener("mouseup", stop); }
  /** 使用系统目录选择器；E2E 构建只替换无法由 WebDriver 操作的系统窗口返回值。 */
  async function chooseFolder() {
    const e2ePath = (window as Window & { __NARRANT_E2E_DIRECTORY__?: string }).__NARRANT_E2E_DIRECTORY__;
    const path = import.meta.env.VITE_WDIO === "true" && e2ePath
      ? e2ePath
      : await open({ directory: true, multiple: false });
    if (typeof path === "string") onOpenFolder(path);
  }
  return <SidebarProvider open={openSidebar} onOpenChange={(open) => setPreferences((value: object) => ({ ...value, open }))} style={{ "--sidebar-width": `${sidebarWidth}px` } as CSSProperties}>
    <Sidebar aria-label={t("sidebar")} collapsible="icon">
      <SidebarContent><SidebarGroup><SidebarGroupLabel>{t("recent")}</SidebarGroupLabel><SidebarGroupContent><SidebarMenu>
        {reviews.slice(0, 10).map((review) => <SidebarMenuItem key={review.reviewId}><SidebarMenuButton onClick={() => onOpenReview(review)}><span>{review.repositoryPath.split("/").at(-1)} · {review.baseRef.split("/").at(-1)} → {review.headRef.split("/").at(-1)}</span></SidebarMenuButton></SidebarMenuItem>)}
        {!reviews.length && <p className="px-2 text-sm text-muted-foreground">{t("noRecent")}</p>}
      </SidebarMenu></SidebarGroupContent></SidebarGroup></SidebarContent>
      <div aria-label={t("resizeSidebar")} className="absolute right-0 top-0 h-full w-1 cursor-col-resize" onMouseDown={resizeSidebar} />
    </Sidebar>
    <main className="min-h-screen w-full bg-background p-8"><header className="flex items-center gap-3 border-b pb-5"><SidebarTrigger aria-label={openSidebar ? t("collapse") : t("expand")} /><div><h1 className="font-heading text-xl">{t("brand")}</h1><p className="text-sm text-muted-foreground">{t("subtitle")}</p></div>{reviews.length > 0 && <Button className="ml-auto" onClick={() => void chooseFolder()}><RiFolderOpenLine />{t("openFolder")}</Button>}</header>
      {error && <p role="alert" className="mx-auto mt-6 max-w-3xl rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">{error}</p>}
      {loading
        ? <section role="status" aria-label={t("loadingRecent")} className="mx-auto max-w-3xl space-y-4 py-10"><Skeleton className="h-8 w-40" /><Skeleton className="h-16 w-full" /><Skeleton className="h-16 w-full" /></section>
        : reviews.length ? <section className="mx-auto max-w-3xl py-10" aria-label={t("recent")}><h2 className="font-heading text-2xl">{t("recent")}</h2><ul className="mt-5 divide-y rounded-lg border">{reviews.slice(0, 10).map((review) => <li key={review.reviewId}><Button variant="ghost" className="h-auto w-full justify-start rounded-none py-4" onClick={() => onOpenReview(review)}>{review.repositoryPath} · {review.baseRef.split("/").at(-1)} → {review.headRef.split("/").at(-1)}</Button></li>)}</ul></section> : <section className="mx-auto flex max-w-xl flex-col items-start gap-5 py-24"><h2 className="font-heading text-3xl">{t("emptyTitle")}</h2><p className="text-muted-foreground">{t("emptyDescription")}</p><Button size="lg" onClick={() => void chooseFolder()}><RiFolderOpenLine />{t("openFolder")}</Button></section>}
    </main>
  </SidebarProvider>;
}

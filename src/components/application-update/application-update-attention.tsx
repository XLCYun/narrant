import { AlertCircleIcon, DownloadCircle01Icon, InstallingUpdates01Icon } from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import type { ApplicationUpdateSnapshot } from "@/application-update/application-update";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { cn } from "@/lib/utils";

/** 只为需要用户注意的四种快照状态呈现侧栏圆形入口。 */
export function ApplicationUpdateAttention({
  snapshot,
  onOpen,
}: {
  snapshot?: ApplicationUpdateSnapshot;
  onOpen(): void;
}) {
  const { t } = useTranslation("applicationUpdate");
  if (!snapshot || !["available", "downloading", "failed", "staged"].includes(snapshot.state.kind)) return null;
  const icon = snapshot.state.kind === "failed"
    ? AlertCircleIcon
    : snapshot.state.kind === "staged" ? InstallingUpdates01Icon : DownloadCircle01Icon;
  return <Button
    variant="outline"
    size="icon-sm"
    data-slot="application-update-attention"
    className={cn(
      "size-8 shrink-0 rounded-full",
      snapshot.state.kind === "failed" && "border-destructive/40 text-destructive",
      snapshot.state.kind === "staged" && "border-primary/40 text-primary",
    )}
    aria-label={t(`attention.${snapshot.state.kind}`)}
    onClick={onOpen}
    onKeyDown={(event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      onOpen();
    }}
  >
    <Icon icon={icon} className="size-4" aria-hidden />
  </Button>;
}

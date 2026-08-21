import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Spinner } from "@/components/ui/spinner";

/** Repository Trust 弹层所需的最小状态与操作。 */
type RepositoryTrustDialogProps = {
  open: boolean;
  canonicalPath: string;
  granting: boolean;
  onCancel: () => void;
  onConfirm: () => void;
};

/** 在 AppShell 之外呈现 Repository Trust，确保 Home 与窄窗口共享同一弹层边界。 */
export function RepositoryTrustDialog({ open, canonicalPath, granting, onCancel, onConfirm }: RepositoryTrustDialogProps) {
  const { t } = useTranslation("common");
  return <Dialog open={open} onOpenChange={(nextOpen) => {
    if (!nextOpen && !granting) onCancel();
  }}>
    <DialogContent>
      <DialogHeader>
        <DialogTitle>{t("trustTitle")}</DialogTitle>
        <DialogDescription>{t("trustDescription")}</DialogDescription>
      </DialogHeader>
      <p className="break-all text-sm text-muted-foreground">{canonicalPath}</p>
      <DialogFooter>
        <Button variant="outline" disabled={granting} onClick={onCancel}>{t("cancel")}</Button>
        <Button aria-busy={granting || undefined} disabled={granting} onClick={onConfirm}>
          {granting && <Spinner aria-hidden />}{t("trustAndOpen")}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>;
}

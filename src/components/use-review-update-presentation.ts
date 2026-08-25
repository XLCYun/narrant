import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import type { ReviewReaderBinding } from "@/review/use-review-lifecycle";

/** Review 更新操作向 Reader 暴露的紧凑展示模型。 */
export type ReviewUpdatePresentation = {
  label: string;
  statusLabel?: string;
  statusTone: "neutral" | "warning";
  busy: boolean;
  run(): void;
};

/** 收拢更新状态映射、用户动作选择与非阻断通知。 */
export function useReviewUpdatePresentation(
  review: Pick<ReviewReaderBinding, "update" | "transition" | "checkForUpdate" | "refresh">,
): ReviewUpdatePresentation {
  const { t } = useTranslation("review");
  const refreshing = review.transition.kind === "refreshing";
  const checking = review.update.kind === "checking";
  const refreshAvailable = review.update.kind === "available" || review.update.kind === "refresh-failed";
  const label = refreshing ? t("refreshing")
    : checking ? t("checkingUpdate")
    : review.update.kind === "refresh-failed" ? t("retryRefresh")
    : review.update.kind === "confirmation-stale" ? t("recheck")
    : refreshAvailable ? t("refresh")
    : review.update.kind === "failed" ? t("retryCheck")
    : t("checkUpdate");
  const notifiedFailure = useRef<ReviewReaderBinding["update"]>();

  useEffect(() => {
    if (notifiedFailure.current === review.update) return;
    notifiedFailure.current = review.update;
    if (review.update.kind === "failed") toast.error(t("checkFailed"), { description: review.update.failure.detail });
    if (review.update.kind === "refresh-failed") toast.error(t("refreshFailed"), { description: review.update.failure.detail });
    if (review.update.kind === "confirmation-stale") toast.warning(t("confirmationStale"));
  }, [review.update, t]);

  /** 根据展示状态执行唯一可用的检查或刷新动作。 */
  const run = useCallback(() => {
    if (!refreshAvailable) {
      review.checkForUpdate();
      return;
    }
    void review.refresh().then((result) => {
      if (result.kind === "refreshed") toast.success(t("refreshed"));
    });
  }, [refreshAvailable, review, t]);

  return {
    label,
    statusLabel: review.update.kind === "current" ? t("latest")
      : refreshAvailable ? t("updateAvailable")
      : undefined,
    statusTone: refreshAvailable ? "warning" : "neutral",
    busy: checking || review.transition.kind !== "stable",
    run,
  };
}

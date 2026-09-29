import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import type { ReviewReaderBinding } from "@/review/use-review-lifecycle";

/** Review 更新操作向 Reader 暴露的紧凑展示模型。 */
export type ReviewUpdatePresentation = {
  label: string;
  busy: boolean;
  run(): void;
};

/** 将用户本轮手动检查接续为刷新，并隔离已离开的 Review 请求。 */
export function useReviewUpdatePresentation(
  review: Pick<ReviewReaderBinding, "snapshot" | "update" | "transition" | "checkForUpdate" | "refresh">,
): ReviewUpdatePresentation {
  const { t } = useTranslation("review");
  const [pending, setPending] = useState(false);
  const request = useRef<symbol>();
  const notifiedFailure = useRef<ReviewReaderBinding["update"]>();
  const refreshing = review.transition.kind === "refreshing";
  const busy = pending || review.update.kind === "checking" || review.transition.kind !== "stable";
  const label = refreshing ? t("refreshing") : busy ? t("checkingUpdate") : t("checkUpdate");

  useEffect(() => {
    setPending(false);
    return () => { request.current = undefined; };
  }, [review.snapshot.reviewId, review.snapshot.snapshotId]);

  useEffect(() => {
    if (notifiedFailure.current === review.update) return;
    notifiedFailure.current = review.update;
    if (review.update.kind === "failed") toast.error(t("checkFailed"), { description: review.update.failure.detail });
    if (review.update.kind === "refresh-failed") toast.error(t("refreshFailed"), { description: review.update.failure.detail });
    if (review.update.kind === "confirmation-stale") toast.warning(t("confirmationStale"));
  }, [review.update, t]);

  /** 串行完成检查与刷新，阻止重复点击，并忽略离开旧审核后的检查结果。 */
  const run = useCallback(() => {
    if (busy || request.current) return;
    const token = Symbol();
    request.current = token;
    setPending(true);
    void (async () => {
      try {
        const available = await review.checkForUpdate();
        if (request.current !== token || available === undefined) return;
        if (!available) {
          toast.success(t("latest"));
          return;
        }
        const result = await review.refresh();
        if (result.kind === "refreshed") toast.success(t("refreshed"));
      } finally {
        if (request.current === token) {
          request.current = undefined;
          setPending(false);
        }
      }
    })();
  }, [busy, review, t]);

  return { label, busy, run };
}

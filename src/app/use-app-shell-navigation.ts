import { useCallback, useMemo, useRef, useState } from "react";

import type { ReviewSummary } from "@/lib/review-types";
import type { RepositoryContext, RepositoryEntry } from "@/repository/repository-application";
import type { RepositoryEntryContextBinding } from "@/repository/use-repository-entry-context";
import type { ReviewOpenTarget } from "@/review/review-application";
import type { ReviewLifecycleBinding, ReviewReaderBinding } from "@/review/use-review-lifecycle";

export type RepositoryTab = "pull-requests" | "branches" | "history";

export type AppDestination =
  | { kind: "home" }
  | { kind: "repository"; repositoryEntryId: number; initialTab: RepositoryTab }
  | { kind: "review"; repositoryEntryId: number; reviewId: string };

export type AppNavigationIntent =
  | { kind: "home" }
  | { kind: "repository"; repositoryEntryId: number }
  | { kind: "review"; repositoryEntryId: number; target: ReviewOpenTarget };

export type AppNavigationResult =
  | { kind: "navigated" }
  | { kind: "trust-required" }
  | { kind: "ignored" }
  | { kind: "failed"; detail: string; previousDestinationPreserved: true };

export type AppNavigationTransition =
  | { kind: "stable" }
  | { kind: "opening-repository"; repositoryEntryId: number }
  | { kind: "opening-review"; repositoryEntryId: number; key: string }
  | { kind: "trust-required"; repository: RepositoryEntry }
  | { kind: "granting-trust"; repository: RepositoryEntry };

export type AppShellNavigationBinding = {
  destination: AppDestination;
  transition: AppNavigationTransition;
  reader: ReviewReaderBinding | null;
  sidebar: {
    repositories: RepositoryEntry[];
    reviews: ReviewSummary[];
    activeRepositoryId?: number;
    activeReviewId?: string;
    pendingRepositoryId?: number;
    pendingReviewId?: string;
    busy: boolean;
    loading: boolean;
    error?: string;
  };
  trust: null | { repository: RepositoryEntry; granting: boolean };
  navigate(intent: AppNavigationIntent): Promise<AppNavigationResult>;
  registerAndOpen(path: string): Promise<AppNavigationResult>;
  confirmTrust(): Promise<AppNavigationResult>;
  dismissTrust(): void;
};

type Options = {
  waitForPaint?: () => Promise<void>;
};

function reviewKey(target: ReviewOpenTarget) {
  if (target.kind === "saved-review") return `review:${target.reviewId}`;
  if (target.kind === "branch-selection") return `branch:${target.baseRef}:${target.headRef}`;
  return `${target.owner.toLowerCase()}/${target.repository.toLowerCase()}#${target.number}`;
}

function reviewSummary(context: RepositoryContext, snapshot: ReviewReaderBinding["snapshot"]): ReviewSummary {
  return {
    reviewId: snapshot.reviewId,
    repositoryEntryId: context.repository.id,
    repositoryPath: context.repository.canonicalPath,
    baseRef: snapshot.baseRef,
    headRef: snapshot.headRef,
    lastOpenedAt: Date.now(),
    pullRequest: snapshot.pullRequest,
  };
}

function initialDestination(
  repository: RepositoryEntryContextBinding,
  review: ReviewLifecycleBinding,
): AppDestination {
  if (review.reader && repository.current) {
    return { kind: "review", repositoryEntryId: repository.current.repository.id, reviewId: review.reader.snapshot.reviewId };
  }
  if (repository.current) {
    return { kind: "repository", repositoryEntryId: repository.current.repository.id, initialTab: "pull-requests" };
  }
  return { kind: "home" };
}

/** 协调 Home、Repository Entry 与 Review 之间的原子应用级导航。 */
export function useAppShellNavigation(
  repository: RepositoryEntryContextBinding,
  review: ReviewLifecycleBinding,
  options: Options = {},
): AppShellNavigationBinding {
  const [destination, setDestination] = useState<AppDestination>(() => initialDestination(repository, review));
  const [transition, setTransition] = useState<AppNavigationTransition>({ kind: "stable" });
  const [pendingIntent, setPendingIntent] = useState<Exclude<AppNavigationIntent, { kind: "home" }>>();
  const [pendingPreviousRepositoryId, setPendingPreviousRepositoryId] = useState<number>();
  const [navigationError, setNavigationError] = useState<string>();
  const navigating = useRef(false);
  const waitForPaint = useMemo(() => options.waitForPaint ?? (() => new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => window.requestAnimationFrame(() => resolve()));
  })), [options.waitForPaint]);

  const restoreRepository = useCallback(async (repositoryEntryId?: number) => {
    if (repositoryEntryId === undefined) {
      repository.leave();
      return;
    }
    await repository.open(repositoryEntryId);
  }, [repository]);

  const commitReview = useCallback(async (
    intent: Extract<AppNavigationIntent, { kind: "review" }>,
    context: RepositoryContext,
    previousRepositoryId?: number,
  ): Promise<AppNavigationResult> => {
    const result = await review.open(intent.target);
    if (result.kind !== "opened") {
      if (context.repository.id !== previousRepositoryId) await restoreRepository(previousRepositoryId);
      const detail = result.failure.detail;
      setNavigationError(detail);
      return { kind: "failed", detail, previousDestinationPreserved: true };
    }
    repository.recordReviewOpened(reviewSummary(context, result.snapshot));
    void repository.refreshReviews().catch(() => undefined);
    setDestination({ kind: "review", repositoryEntryId: context.repository.id, reviewId: result.reviewId });
    setNavigationError(undefined);
    return { kind: "navigated" };
  }, [repository, restoreRepository, review]);

  const navigate = useCallback(async (intent: AppNavigationIntent): Promise<AppNavigationResult> => {
    if (navigating.current) return { kind: "ignored" };
    navigating.current = true;
    const previousRepositoryId = repository.current?.repository.id;
    try {
      if (intent.kind === "home") {
        repository.leave();
        setPendingIntent(undefined);
        setPendingPreviousRepositoryId(undefined);
        setDestination({ kind: "home" });
        setTransition({ kind: "stable" });
        setNavigationError(undefined);
        await review.close();
        return { kind: "navigated" };
      }

      if (intent.kind === "repository") {
        setTransition({ kind: "opening-repository", repositoryEntryId: intent.repositoryEntryId });
        const opened = await repository.open(intent.repositoryEntryId);
        if (opened.kind === "superseded") return { kind: "ignored" };
        if (opened.kind === "trust-required") {
          setPendingIntent(intent);
          setPendingPreviousRepositoryId(previousRepositoryId);
          setTransition({ kind: "trust-required", repository: opened.repository });
          return { kind: "trust-required" };
        }
        if (opened.kind === "unavailable") {
          const detail = opened.repository.unavailableReason ?? "Repository Entry 当前不可用。";
          setNavigationError(detail);
          setTransition({ kind: "stable" });
          return { kind: "failed", detail, previousDestinationPreserved: true };
        }
        const initialTab = destination.kind === "review" && previousRepositoryId === opened.context.repository.id
          ? "history"
          : "pull-requests";
        await review.close();
        setDestination({ kind: "repository", repositoryEntryId: opened.context.repository.id, initialTab });
        setTransition({ kind: "stable" });
        setNavigationError(undefined);
        return { kind: "navigated" };
      }

      setTransition({ kind: "opening-review", repositoryEntryId: intent.repositoryEntryId, key: reviewKey(intent.target) });
      await waitForPaint();
      let context = repository.current;
      if (intent.repositoryEntryId === 0 && !context) {
        const result = await review.open(intent.target);
        if (result.kind !== "opened") {
          const detail = result.failure.detail;
          setNavigationError(detail);
          setTransition({ kind: "stable" });
          return { kind: "failed", detail, previousDestinationPreserved: true };
        }
        setDestination({ kind: "review", repositoryEntryId: 0, reviewId: result.reviewId });
        setNavigationError(undefined);
        setTransition({ kind: "stable" });
        return { kind: "navigated" };
      }
      if (context?.repository.id !== intent.repositoryEntryId) {
        const opened = await repository.open(intent.repositoryEntryId);
        if (opened.kind === "superseded") return { kind: "ignored" };
        if (opened.kind === "trust-required") {
          setPendingIntent(intent);
          setPendingPreviousRepositoryId(previousRepositoryId);
          setTransition({ kind: "trust-required", repository: opened.repository });
          return { kind: "trust-required" };
        }
        if (opened.kind === "unavailable") {
          const detail = opened.repository.unavailableReason ?? "Repository Entry 当前不可用。";
          setNavigationError(detail);
          setTransition({ kind: "stable" });
          return { kind: "failed", detail, previousDestinationPreserved: true };
        }
        context = opened.context;
      }
      const result = await commitReview(intent, context, previousRepositoryId);
      setTransition({ kind: "stable" });
      return result;
    } catch (reason) {
      const detail = String(reason);
      setNavigationError(detail);
      setTransition({ kind: "stable" });
      return { kind: "failed", detail, previousDestinationPreserved: true };
    } finally {
      navigating.current = false;
    }
  }, [commitReview, destination.kind, repository, review, waitForPaint]);

  const registerAndOpen = useCallback(async (path: string) => {
    try {
      const entry = await repository.register(path);
      return await navigate({ kind: "repository", repositoryEntryId: entry.id });
    } catch (reason) {
      const detail = String(reason);
      setNavigationError(detail);
      return { kind: "failed", detail, previousDestinationPreserved: true } as const;
    }
  }, [navigate, repository]);

  const confirmTrust = useCallback(async (): Promise<AppNavigationResult> => {
    if (!pendingIntent || transition.kind !== "trust-required" || navigating.current) return { kind: "ignored" };
    navigating.current = true;
    const trustedRepository = transition.repository;
    setTransition({ kind: "granting-trust", repository: trustedRepository });
    try {
      const context = await repository.confirmTrust();
      if (!context) return { kind: "ignored" };
      const result = pendingIntent.kind === "review"
        ? await commitReview(pendingIntent, context, pendingPreviousRepositoryId)
        : await (async (): Promise<AppNavigationResult> => {
          const initialTab = destination.kind === "review" && pendingPreviousRepositoryId === context.repository.id
            ? "history"
            : "pull-requests";
          await review.close();
          setDestination({ kind: "repository", repositoryEntryId: context.repository.id, initialTab });
          setNavigationError(undefined);
          return { kind: "navigated" };
        })();
      setPendingIntent(undefined);
      setPendingPreviousRepositoryId(undefined);
      setTransition({ kind: "stable" });
      return result;
    } catch (reason) {
      const detail = String(reason);
      setNavigationError(detail);
      setTransition({ kind: "trust-required", repository: trustedRepository });
      return { kind: "failed", detail, previousDestinationPreserved: true };
    } finally {
      navigating.current = false;
    }
  }, [commitReview, destination.kind, pendingIntent, pendingPreviousRepositoryId, repository, review, transition]);

  const dismissTrust = useCallback(() => {
    repository.dismissTrust();
    setPendingIntent(undefined);
    setPendingPreviousRepositoryId(undefined);
    setTransition({ kind: "stable" });
  }, [repository]);

  const pendingReviewId = transition.kind === "opening-review" && transition.key.startsWith("review:")
    ? transition.key.slice("review:".length)
    : undefined;
  const activeRepositoryId = destination.kind === "home" ? undefined : destination.repositoryEntryId;
  const activeReviewId = destination.kind === "review" ? destination.reviewId : undefined;
  const busy = transition.kind !== "stable" || review.reader?.transition.kind === "refreshing";
  const trust = useMemo(() => transition.kind === "trust-required" || transition.kind === "granting-trust"
    ? { repository: transition.repository, granting: transition.kind === "granting-trust" }
    : null, [transition]);

  return useMemo(() => ({
    destination,
    transition,
    reader: review.reader,
    sidebar: {
      repositories: repository.catalog.repositories,
      reviews: repository.catalog.recentReviews,
      activeRepositoryId,
      activeReviewId,
      pendingRepositoryId: transition.kind === "opening-repository" ? transition.repositoryEntryId : undefined,
      pendingReviewId,
      busy,
      loading: repository.catalog.status === "loading" || repository.transition.kind === "registering",
      error: navigationError ?? repository.catalog.error,
    },
    trust,
    navigate,
    registerAndOpen,
    confirmTrust,
    dismissTrust,
  }), [activeRepositoryId, activeReviewId, busy, confirmTrust, destination, dismissTrust, navigate, navigationError, pendingReviewId, registerAndOpen, repository.catalog, repository.transition.kind, review.reader, transition, trust]);
}

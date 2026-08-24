import { useCallback, useMemo, useRef, useState } from "react";

import type { RecentReviewSummary, RepositoryReviewSummary } from "@/lib/review-types";
import type { RepositoryOverview, RepositorySummary } from "@/repository/repository-application";
import type { RepositoryOverviewBinding } from "@/repository/use-repository-overview";
import { knownReviewId, type ReviewOpenTarget } from "@/review/review-application";
import type { ReviewLifecycleBinding, ReviewReaderBinding } from "@/review/use-review-lifecycle";
import type { SettingsSection } from "@/settings/settings-section";

export type RepositoryTab = "pull-requests" | "branches" | "history";

export type AppDestination =
  | { kind: "home" }
  | { kind: "repository"; repositoryId: number; initialTab: RepositoryTab }
  | { kind: "review"; repositoryId: number; reviewId: string }
  | { kind: "settings"; initialSection?: SettingsSection };

export type AppNavigationIntent =
  | { kind: "home" }
  | { kind: "settings"; initialSection?: SettingsSection }
  | { kind: "repository"; repositoryId: number }
  | { kind: "review"; repositoryId: number; target: ReviewOpenTarget };

export type AppNavigationResult =
  | { kind: "navigated" }
  | { kind: "trust-required" }
  | { kind: "ignored" }
  | { kind: "failed"; detail: string; previousDestinationPreserved: true };

export type AppNavigationTransition =
  | { kind: "stable" }
  | { kind: "opening-repository"; repositoryId: number }
  | { kind: "opening-review"; repositoryId: number; key: string }
  | { kind: "trust-required"; repository: RepositorySummary }
  | { kind: "granting-trust"; repository: RepositorySummary };

export type AppShellNavigationBinding = {
  destination: AppDestination;
  transition: AppNavigationTransition;
  reader: ReviewReaderBinding | null;
  sidebar: {
    repositories: RepositorySummary[];
    reviews: RecentReviewSummary[];
    activeRepositoryId?: number;
    activeReviewId?: string;
    pendingRepositoryId?: number;
    pendingReviewId?: string;
    busy: boolean;
    loading: boolean;
    error?: string;
  };
  trust: null | { repository: RepositorySummary; granting: boolean };
  navigate(intent: AppNavigationIntent): Promise<AppNavigationResult>;
  returnFromSettings(): Promise<AppNavigationResult>;
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

function recentReviewSummary(overview: RepositoryOverview, snapshot: ReviewReaderBinding["snapshot"]): RecentReviewSummary {
  const selection: RepositoryReviewSummary["selection"] = snapshot.pullRequest
    ? { kind: "pull-request", baseRef: snapshot.baseRef, headRef: snapshot.headRef, pullRequest: snapshot.pullRequest }
    : { kind: "branch", baseRef: snapshot.baseRef, headRef: snapshot.headRef };
  return {
    repositoryId: overview.repository.id,
    repositoryPath: overview.repository.canonicalPath,
    review: {
    reviewId: snapshot.reviewId,
    selection,
    currentSnapshot: {
      baseRevision: snapshot.baseRevision,
      comparisonBase: snapshot.comparisonBase,
      headRevision: snapshot.headRevision,
    },
    lastOpenedAt: Date.now(),
    },
  };
}

function initialDestination(
  repository: RepositoryOverviewBinding,
  review: ReviewLifecycleBinding,
): AppDestination {
  if (review.reader && repository.current) {
    return { kind: "review", repositoryId: repository.current.repository.id, reviewId: review.reader.snapshot.reviewId };
  }
  if (repository.current) {
    return { kind: "repository", repositoryId: repository.current.repository.id, initialTab: "pull-requests" };
  }
  return { kind: "home" };
}

/** 协调 Home、Repository 与 Review 之间的原子应用级导航。 */
export function useAppShellNavigation(
  repository: RepositoryOverviewBinding,
  review: ReviewLifecycleBinding,
  options: Options = {},
): AppShellNavigationBinding {
  const [destination, setDestination] = useState<AppDestination>(() => initialDestination(repository, review));
  const [transition, setTransition] = useState<AppNavigationTransition>({ kind: "stable" });
  const [pendingIntent, setPendingIntent] = useState<Exclude<AppNavigationIntent, { kind: "home" }>>();
  const [pendingPreviousRepositoryId, setPendingPreviousRepositoryId] = useState<number>();
  const [navigationError, setNavigationError] = useState<string>();
  const settingsReturnDestination = useRef<Exclude<AppDestination, { kind: "settings" }>>({ kind: "home" });
  const navigating = useRef(false);
  const waitForPaint = useMemo(() => options.waitForPaint ?? (() => new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => window.requestAnimationFrame(() => resolve()));
  })), [options.waitForPaint]);

  const restoreRepository = useCallback(async (repositoryId?: number) => {
    if (repositoryId === undefined) {
      repository.leave();
      return;
    }
    await repository.open(repositoryId);
  }, [repository]);

  const commitReview = useCallback(async (
    intent: Extract<AppNavigationIntent, { kind: "review" }>,
    overview: RepositoryOverview,
    previousRepositoryId?: number,
  ): Promise<AppNavigationResult> => {
    const result = await review.open(intent.target);
    if (result.kind !== "opened") {
      if (overview.repository.id !== previousRepositoryId) await restoreRepository(previousRepositoryId);
      const detail = result.failure.detail;
      setNavigationError(detail);
      return { kind: "failed", detail, previousDestinationPreserved: true };
    }
    repository.recordReviewOpened(recentReviewSummary(overview, result.snapshot));
    setDestination({ kind: "review", repositoryId: overview.repository.id, reviewId: result.reviewId });
    setNavigationError(undefined);
    return { kind: "navigated" };
  }, [repository, restoreRepository, review]);

  const navigate = useCallback(async (intent: AppNavigationIntent): Promise<AppNavigationResult> => {
    const requestedReviewId = intent.kind === "review" ? knownReviewId(intent.target) : undefined;
    if (intent.kind === "review"
      && requestedReviewId
      && destination.kind === "review"
      && destination.repositoryId === intent.repositoryId
      && destination.reviewId === requestedReviewId) {
      return { kind: "navigated" };
    }
    if (navigating.current) return { kind: "ignored" };
    navigating.current = true;
    const previousRepositoryId = repository.current?.repository.id;
    try {
      if (intent.kind === "settings") {
        if (destination.kind !== "settings") settingsReturnDestination.current = destination;
        setDestination({ kind: "settings", initialSection: intent.initialSection });
        setTransition({ kind: "stable" });
        setNavigationError(undefined);
        return { kind: "navigated" };
      }

      if (intent.kind === "home") {
        repository.leave();
        setPendingIntent(undefined);
        setPendingPreviousRepositoryId(undefined);
        setDestination({ kind: "home" });
        setTransition({ kind: "stable" });
        setNavigationError(undefined);
        review.unpresent();
        return { kind: "navigated" };
      }

      if (intent.kind === "repository") {
        setTransition({ kind: "opening-repository", repositoryId: intent.repositoryId });
        const opened = await repository.open(intent.repositoryId);
        if (opened.kind === "superseded") return { kind: "ignored" };
        if (opened.kind === "trust-required") {
          setPendingIntent(intent);
          setPendingPreviousRepositoryId(previousRepositoryId);
          setTransition({ kind: "trust-required", repository: opened.repository });
          return { kind: "trust-required" };
        }
        if (opened.kind === "unavailable") {
          const detail = opened.repository.availability.kind === "unavailable" ? opened.repository.availability.reason : "Repository 当前不可用。";
          setNavigationError(detail);
          setTransition({ kind: "stable" });
          return { kind: "failed", detail, previousDestinationPreserved: true };
        }
        const initialTab = destination.kind === "review" && previousRepositoryId === opened.overview.repository.id
          ? "history"
          : "pull-requests";
        review.unpresent();
        setDestination({ kind: "repository", repositoryId: opened.overview.repository.id, initialTab });
        setTransition({ kind: "stable" });
        setNavigationError(undefined);
        return { kind: "navigated" };
      }

      setTransition({ kind: "opening-review", repositoryId: intent.repositoryId, key: reviewKey(intent.target) });
      await waitForPaint();
      let overview = repository.current;
      if (intent.repositoryId === 0 && !overview) {
        const result = await review.open(intent.target);
        if (result.kind !== "opened") {
          const detail = result.failure.detail;
          setNavigationError(detail);
          setTransition({ kind: "stable" });
          return { kind: "failed", detail, previousDestinationPreserved: true };
        }
        setDestination({ kind: "review", repositoryId: 0, reviewId: result.reviewId });
        setNavigationError(undefined);
        setTransition({ kind: "stable" });
        return { kind: "navigated" };
      }
      if (overview?.repository.id !== intent.repositoryId) {
        const opened = await repository.open(intent.repositoryId);
        if (opened.kind === "superseded") return { kind: "ignored" };
        if (opened.kind === "trust-required") {
          setPendingIntent(intent);
          setPendingPreviousRepositoryId(previousRepositoryId);
          setTransition({ kind: "trust-required", repository: opened.repository });
          return { kind: "trust-required" };
        }
        if (opened.kind === "unavailable") {
          const detail = opened.repository.availability.kind === "unavailable" ? opened.repository.availability.reason : "Repository 当前不可用。";
          setNavigationError(detail);
          setTransition({ kind: "stable" });
          return { kind: "failed", detail, previousDestinationPreserved: true };
        }
        overview = opened.overview;
      }
      const result = await commitReview(intent, overview, previousRepositoryId);
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
  }, [commitReview, destination, repository, review, waitForPaint]);

  /** 离开 Application Settings，并恢复进入前已提交的应用目的地。 */
  const returnFromSettings = useCallback(async (): Promise<AppNavigationResult> => {
    if (destination.kind !== "settings") return { kind: "ignored" };
    setDestination(settingsReturnDestination.current);
    return { kind: "navigated" };
  }, [destination.kind]);

  const registerAndOpen = useCallback(async (path: string) => {
    try {
      const entry = await repository.register(path);
      return await navigate({ kind: "repository", repositoryId: entry.id });
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
      const overview = await repository.confirmTrust();
      if (!overview) return { kind: "ignored" };
      const result = pendingIntent.kind === "review"
        ? await commitReview(pendingIntent, overview, pendingPreviousRepositoryId)
        : await (async (): Promise<AppNavigationResult> => {
          const initialTab = destination.kind === "review" && pendingPreviousRepositoryId === overview.repository.id
            ? "history"
            : "pull-requests";
          review.unpresent();
          setDestination({ kind: "repository", repositoryId: overview.repository.id, initialTab });
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
  const activeRepositoryId = destination.kind === "repository" || destination.kind === "review"
    ? destination.repositoryId
    : undefined;
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
      repositories: repository.navigation.repositories,
      reviews: repository.navigation.recentReviews,
      activeRepositoryId,
      activeReviewId,
      pendingRepositoryId: transition.kind === "opening-repository" ? transition.repositoryId : undefined,
      pendingReviewId,
      busy,
      loading: repository.navigation.status === "loading" || repository.transition.kind === "registering",
      error: navigationError ?? repository.navigation.error,
    },
    trust,
    navigate,
    returnFromSettings,
    registerAndOpen,
    confirmTrust,
    dismissTrust,
  }), [activeRepositoryId, activeReviewId, busy, confirmTrust, destination, dismissTrust, navigate, navigationError, pendingReviewId, registerAndOpen, repository.navigation, repository.transition.kind, returnFromSettings, review.reader, transition, trust]);
}

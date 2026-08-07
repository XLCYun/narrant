import { useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useSystemTheme } from "@/lib/use-system-theme";
import { AppShell } from "@/components/home-page";
import { RepositoryOverview } from "@/components/repository-overview";
import { BranchRelation } from "@/components/branch-relation";
import { BranchReviewSelector, type BranchSelectorAction } from "@/components/branch-review-selector";
import { GitHubIdentityEntry } from "@/components/github-identity-entry";
import { GitHubAuthorizationControls } from "@/components/github-authorization-controls";
import { Spinner } from "@/components/ui/spinner";
import { Toaster } from "@/components/ui/sonner";
import {
  ReviewReader,
  type DiffMode,
} from "@/components/review-reader";
import type { DiscoveredPullRequest, PullRequestGroup, ReviewSummary, SavedReviewSummary } from "@/lib/review-types";
import { createTauriReviewApplicationAdapter } from "@/infrastructure/tauri-review-application-adapter";
import { createTauriRepositoryApplicationAdapter } from "@/infrastructure/tauri-repository-application-adapter";
import { createTauriGitHubOAuthApplicationAdapter } from "@/infrastructure/tauri-github-oauth-application-adapter";
import { createTauriPullRequestDiscoveryApplicationAdapter } from "@/infrastructure/tauri-pull-request-discovery-application-adapter";
import {
  tauriTransport,
} from "@/infrastructure/tauri/transport";
import type { BranchSelectionUpdate } from "@/review/review-application";
import { useGitHubIdentityLifecycle } from "@/github/use-github-identity-lifecycle";
import { usePullRequestDiscoveryLifecycle } from "@/github/use-pull-request-discovery-lifecycle";
import { useReviewLifecycle } from "@/review/use-review-lifecycle";
import { useRepositoryEntryContext } from "@/repository/use-repository-entry-context";
import { useAppShellNavigation } from "@/app/use-app-shell-navigation";
import "@/lib/i18n";

/** Repository Entry 中已有 Branch Selection 的更新检查状态。 */
type BranchSelectionCheck =
  | { status: "idle" | "checking" | "current" }
  | { status: "update"; update: BranchSelectionUpdate }
  | { status: "failed" };
const REVIEW_TREE_WIDTH_KEY = "narrant.review.fileTreeWidth";
const REVIEW_DIFF_MODE_KEY = "narrant.review.diffMode";

function ErrorMessage({ message }: { message: string }) {
  const { t } = useTranslation("review");
  const match = message.match(/(https:\/\/github\.com\/[^\s]+)/);
  const text = match ? message.replace(match[1], "") : message;
  return <p role="alert">
    {text}
    {match && <> <a href={match[1]} onClick={(event) => { event.preventDefault(); void openUrl(match[1]); }}>{t("viewOnGitHub")}</a></>}
  </p>;
}

export function App() {
  const { t } = useTranslation(["common", "review"]);
  const catalogRuntimeEnabled = "__TAURI_INTERNALS__" in window;
  const testHarnessEnabled = import.meta.env.MODE === "test";
  const editorTheme = useSystemTheme();
  const pullRequestDiscovery = useMemo(
    () => createTauriPullRequestDiscoveryApplicationAdapter(tauriTransport),
    [],
  );
  const reviewApplicationAdapter = useMemo(
    () => createTauriReviewApplicationAdapter(tauriTransport),
    [],
  );
  const repositoryApplication = useMemo(
    () => createTauriRepositoryApplicationAdapter(tauriTransport),
    [],
  );
  const githubOAuthApplication = useMemo(
    () => createTauriGitHubOAuthApplicationAdapter(tauriTransport),
    [],
  );
  const repositoryContext = useRepositoryEntryContext(repositoryApplication, { enabled: catalogRuntimeEnabled });
  const reviewLifecycle = useReviewLifecycle(reviewApplicationAdapter);
  const appShell = useAppShellNavigation(repositoryContext, reviewLifecycle);
  const selectedRepository = repositoryContext.current?.repository;
  const githubIdentity = useGitHubIdentityLifecycle(githubOAuthApplication, { enabled: catalogRuntimeEnabled });
  const pullRequestDiscoveryLifecycle = usePullRequestDiscoveryLifecycle(pullRequestDiscovery, {
    enabled: Boolean(selectedRepository),
    repositoryEntryId: selectedRepository?.id,
    signedIn: githubIdentity.status?.signedIn ?? false,
  });
  const snapshot = reviewLifecycle.reader?.snapshot;
  const refreshingReview = reviewLifecycle.reader?.transition.kind === "refreshing";
  const [repositoryPath, setRepositoryPath] = useState("");
  const [baseRef, setBaseRef] = useState("");
  const [headRef, setHeadRef] = useState("");
  const [branchSelectionCheck, setBranchSelectionCheck] = useState<BranchSelectionCheck>({ status: "idle" });
  const [showBranchCheckingMessage, setShowBranchCheckingMessage] = useState(false);
  const branchSelectionCheckId = useRef(0);
  const [error, setError] = useState("");
  const [githubDialogOpen, setGitHubDialogOpen] = useState(false);
  const [diffMode, setDiffMode] = useState<DiffMode>(() => (
    localStorage.getItem(REVIEW_DIFF_MODE_KEY) === "inline" ? "inline" : "side-by-side"
  ));
  const [narrowReview, setNarrowReview] = useState(() => window.innerWidth < 1200);
  const savedTreeWidth = Number(localStorage.getItem(REVIEW_TREE_WIDTH_KEY));
  const initialTreeWidth = Number.isFinite(savedTreeWidth) && savedTreeWidth >= 15 && savedTreeWidth <= 45
    ? savedTreeWidth
    : 24;
  const repositoryEntries = repositoryContext.catalog.repositories;
  const recentReviews = repositoryContext.catalog.recentReviews;
  const savedReviews = repositoryContext.current?.savedReviews ?? [];
  const repositoryReviews = repositoryContext.current?.reviews ?? [];
  const pendingRepository = appShell.trust?.repository;
  const grantingTrust = appShell.trust?.granting ?? false;
  const catalogLoading = repositoryContext.catalog.status === "loading"
    || repositoryContext.transition.kind === "registering";
  const branchState = repositoryContext.branches;
  const refs = branchState.kind === "ready" ? branchState.value.refs : [];
  const defaultRef = branchState.kind === "ready" ? branchState.value.defaultRef : "";
  const currentRef = branchState.kind === "ready" ? branchState.value.currentRef : "";
  const branchesReloading = branchState.kind === "loading";
  const branchReloadError = branchState.kind === "failed" ? branchState.error : "";
  const authStatus = githubIdentity.status;
  const pullRequestGroups = pullRequestDiscoveryLifecycle.groups;
  const discoveryLoading = pullRequestDiscoveryLifecycle.transition.kind === "loading"
    || pullRequestDiscoveryLifecycle.transition.kind === "refreshing";
  const discoveryError = pullRequestDiscoveryLifecycle.error?.detail;
  const visibleError = error || appShell.sidebar.error || "";
  const reviewOpeningKey = appShell.transition.kind === "opening-review" ? appShell.transition.key : undefined;

  /** 把后端技术错误归类为稳定、可翻译的仓库打开摘要，并保留原始详情。 */
  function repositoryOpenError(reason: unknown) {
    const detail = String(reason);
    const key = detail.includes("不是兼容的 working-tree Git 仓库")
      ? "repositoryNotGit"
      : detail.includes("路径不存在或不可访问")
        ? "repositoryUnavailable"
        : "repositoryRegistrationFailed";
    return `${t(key)} ${detail}`;
  }

  useEffect(() => {
    /** 窗口变窄时只临时切换显示模式，不覆盖用户保存的 Diff 偏好。 */
    const updateReviewWidth = () => setNarrowReview(window.innerWidth < 1200);
    window.addEventListener("resize", updateReviewWidth);
    return () => window.removeEventListener("resize", updateReviewWidth);
  }, []);

  /** 保存用户明确选择的宽窗口 Diff 模式。 */
  function selectDiffMode(mode: DiffMode) {
    setDiffMode(mode);
    localStorage.setItem(REVIEW_DIFF_MODE_KEY, mode);
  }

  useEffect(() => {
    if (branchState.kind !== "ready") return;
    setBaseRef(branchState.value.defaultRef);
    setHeadRef(branchState.value.currentRef);
  }, [branchState]);

  /** 验证并持久化用户输入的具体本地 clone。 */
  async function registerRepository() {
    setError("");
    try {
      await repositoryContext.register(repositoryPath);
    } catch (reason) {
      setError(String(reason));
    }
  }

  /** 原生目录选择后的 Repository Entry 注册入口。 */
  async function registerRepositoryFolder(path: string) {
    setRepositoryPath(path);
    setError("");
    const result = await appShell.registerAndOpen(path);
    if (result.kind === "failed") setError(repositoryOpenError(result.detail));
    return result;
  }

  /** Repository context 提交后启动独立的页面数据读取。 */
  useEffect(() => {
    if (!selectedRepository) return;
    setRepositoryPath(selectedRepository.canonicalPath);
    if (!testHarnessEnabled) void repositoryContext.reloadBranches();
    // 页面数据只在已提交的 Repository Entry identity 改变后重新读取。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedRepository?.id]);

  /** 不依赖 GitHub 登录或 discovery，从 cached current Snapshot 重开 Review。 */
  async function openSavedReview(review: SavedReviewSummary) {
    if (!selectedRepository) return;
    setError("");
    const result = await appShell.navigate({
      kind: "review",
      repositoryEntryId: selectedRepository.id,
      target: { kind: "saved-review", reviewId: review.reviewId },
    });
    if (result.kind === "failed") setError(result.detail);
  }

  /** 侧边栏 Review 导航统一处理跨仓库、Trust 与失败回滚。 */
  async function navigateToReview(review: ReviewSummary) {
    setError("");
    const result = await appShell.navigate({
      kind: "review",
      repositoryEntryId: review.repositoryEntryId,
      target: { kind: "saved-review", reviewId: review.reviewId },
    });
    if (result.kind === "failed") setError(result.detail);
  }

  /** 将 discovery metadata 作为 Selection，并让代码内容完全走本地 Core Git。 */
  async function openDiscoveredPullRequest(
    repository: PullRequestGroup["repository"],
    pullRequest: DiscoveredPullRequest,
  ) {
    if (!selectedRepository) return;
    setError("");
    const result = await appShell.navigate({
      kind: "review",
      repositoryEntryId: selectedRepository.id,
      target: {
        kind: "pull-request-selection",
        repositoryEntryId: selectedRepository.id,
        owner: repository.owner,
        repository: repository.name,
        number: pullRequest.number,
      },
    });
    if (result.kind === "failed") setError(result.detail);
  }

  /** 启动连接向导，并在已配置时将用户带到 GitHub App 安装页。 */
  async function connectGitHub() {
    setError("");
    if (!testHarnessEnabled) setGitHubDialogOpen(true);
    if (authStatus?.configured && !authStatus.signedIn && authStatus.appInstallUrl) {
      await openUrl(authStatus.appInstallUrl);
    }
  }

  async function signOutGitHub() {
    await githubIdentity.signOut();
  }

  /** 复用独立的 GitHub Identity 展示适配器，不在 App 重建 lifecycle 流程。 */
  function renderGitHubControls() {
    return <GitHubAuthorizationControls
      identity={githubIdentity}
      onConnect={() => void connectGitHub()}
      onAuthorized={() => setGitHubDialogOpen(false)}
    />;
  }

  /** 在主侧栏和旧布局中复用同一个 GitHub Identity 全局入口。 */
  function renderGitHubIdentityEntry(className = "h-auto min-h-12 w-full justify-start py-2") {
    const entryError = githubIdentity.error?.operation === "load-status"
      || githubIdentity.error?.operation === "sign-out"
      ? githubIdentity.error
      : undefined;
    return <GitHubIdentityEntry
      className={className}
      signedIn={authStatus?.signedIn ?? false}
      identity={authStatus?.identity}
      error={entryError?.detail}
      onConnect={() => void connectGitHub()}
      onSignOut={() => void signOutGitHub()}
      onRetry={() => void (entryError?.operation === "sign-out" ? signOutGitHub() : githubIdentity.retry())}
    />;
  }

  /** 加载 canonical refs，并使用后端识别的默认分支和当前检出分支初始化选择。 */
  async function loadRefs(repositoryEntryId = selectedRepository?.id) {
    const resolvedRepositoryEntryId = repositoryEntryId ?? (testHarnessEnabled ? 0 : undefined);
    if (resolvedRepositoryEntryId === undefined) return;
    try {
      await repositoryContext.reloadBranches(resolvedRepositoryEntryId);
    } catch (reason) {
      setError(String(reason));
    }
  }

  const matchingBranchReview = repositoryReviews.find(
    (review) => review.baseRef === baseRef && review.headRef === headRef,
  );
  const branchSelectionInvalid = Boolean(baseRef && headRef && baseRef === headRef);

  useEffect(() => {
    const checkId = ++branchSelectionCheckId.current;
    setShowBranchCheckingMessage(false);
    if (!matchingBranchReview || branchSelectionInvalid) {
      setBranchSelectionCheck({ status: "idle" });
      return;
    }
    setBranchSelectionCheck({ status: "checking" });
    const messageTimer = window.setTimeout(() => {
      if (branchSelectionCheckId.current === checkId) setShowBranchCheckingMessage(true);
    }, 1000);
    void reviewApplicationAdapter.checkBranchSelectionUpdate(
      matchingBranchReview.reviewId,
      new AbortController().signal,
    ).then((update) => {
      if (branchSelectionCheckId.current !== checkId) return;
      setBranchSelectionCheck(update.changed ? { status: "update", update } : { status: "current" });
    }).catch(() => {
      if (branchSelectionCheckId.current === checkId) setBranchSelectionCheck({ status: "failed" });
    }).finally(() => window.clearTimeout(messageTimer));
    return () => window.clearTimeout(messageTimer);
    // 后台检查由每次调用生成独立 identity；这里只由用户可见的 Selection 驱动。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [matchingBranchReview?.reviewId, baseRef, headRef, branchSelectionInvalid]);

  const branchSelectorAction: BranchSelectorAction = branchSelectionInvalid
    ? "invalid"
    : !matchingBranchReview
      ? "start"
      : branchSelectionCheck.status === "checking"
        ? "checking"
        : branchSelectionCheck.status === "update"
          ? "update"
          : branchSelectionCheck.status === "failed" ? "fallback" : "continue";

  async function openComparison() {
    setError("");
    const target = {
      kind: "branch-selection" as const,
      repositoryEntryId: selectedRepository?.id ?? 0,
      baseRef,
      headRef,
    };
    const result = await appShell.navigate({ kind: "review", repositoryEntryId: selectedRepository?.id ?? 0, target });
    if (result.kind === "failed") setError(result.detail);
  }

  /** 根据已确认的 moving refs 创建新 Snapshot，再进入同一个 durable Review。 */
  async function updateAndOpenBranchReview() {
    if (!matchingBranchReview || branchSelectionCheck.status !== "update" || !selectedRepository) return;
    setError("");
    const result = await appShell.navigate({
      kind: "review",
      repositoryEntryId: selectedRepository.id,
      target: {
        kind: "branch-selection",
        repositoryEntryId: selectedRepository.id,
        baseRef,
        headRef,
        refreshExisting: true,
        existingReviewId: matchingBranchReview.reviewId,
      },
    });
    if (result.kind === "failed") setError(result.detail);
  }

  /** 统一将完整 object id 压缩为用户可辨认的短 revision。 */
  return <AppShell
    navigation={{
      ...appShell,
      registerAndOpen: registerRepositoryFolder,
      sidebar: {
        ...appShell.sidebar,
        error: !selectedRepository ? visibleError || undefined : undefined,
      },
    }}
    enabled={catalogRuntimeEnabled && (!testHarnessEnabled || Boolean(selectedRepository))}
    fallbackClassName="narrant-app"
    githubIdentityEntry={renderGitHubIdentityEntry()}
  >
    <Toaster position="top-right" />
    {(!catalogRuntimeEnabled || testHarnessEnabled) && <header><h1>{t("brand")}</h1><p>{t("subtitle")}</p></header>}
    {!testHarnessEnabled && !catalogRuntimeEnabled && <div className="fixed bottom-4 left-4 z-40 w-56 rounded-xl border bg-background/95 p-2 shadow-lg backdrop-blur">
      {renderGitHubIdentityEntry()}
    </div>}
    {catalogRuntimeEnabled && !selectedRepository && testHarnessEnabled && <article aria-label="Repository Catalog">
      <h2>Repository Entries</h2>
      <section aria-label="添加 Repository" className="flex flex-wrap items-end gap-3">
        <label>Repository 路径<Input aria-label="Repository 路径" value={repositoryPath} onChange={(event) => setRepositoryPath(event.target.value)} /></label>
        <Button disabled={!repositoryPath || catalogLoading} onClick={registerRepository}>
          {catalogLoading ? "正在读取…" : "添加 Repository"}
        </Button>
      </section>
      {repositoryEntries.length === 0
        ? <p>{catalogLoading ? "正在加载 Repository Entries…" : "尚未添加 Repository Entry"}</p>
        : <ul aria-label="Repository Entries">
          {repositoryEntries.map((entry) => <li key={entry.id}>
            <Button variant="outline" disabled={!entry.available} onClick={() => void appShell.navigate({ kind: "repository", repositoryEntryId: entry.id })}>
              {entry.canonicalPath}
            </Button>
            {!entry.available && <>
              <strong>不可用</strong>
              <p>{entry.unavailableReason}</p>
            </>}
          </li>)}
        </ul>}
      <section aria-label="Recent Reviews">
        <h3>Recent Reviews</h3>
        {recentReviews.length === 0
          ? <p>尚无最近 Review</p>
          : <ul>{recentReviews.map((review) => <li key={review.reviewId}>
            <Button variant="outline" disabled={Boolean(reviewOpeningKey)} aria-busy={reviewOpeningKey === `review:${review.reviewId}` || undefined} onClick={() => void navigateToReview(review)}>
              {reviewOpeningKey === `review:${review.reviewId}` && <Spinner aria-hidden />}
              <span className="mr-3">{review.repositoryPath}</span><BranchRelation baseRef={review.baseRef} headRef={review.headRef} interactive={false} />
            </Button>
          </li>)}</ul>}
      </section>
    </article>}
    {selectedRepository && !snapshot && <article aria-label="Repository Entry">
      <RepositoryOverview
        repositoryName={selectedRepository.canonicalPath.split(/[\\/]/).filter(Boolean).at(-1) ?? selectedRepository.canonicalPath}
        repositoryPath={selectedRepository.canonicalPath}
        savedReviews={savedReviews}
        pullRequestGroups={pullRequestGroups}
        repositoryReviews={repositoryReviews}
        discoveryLoading={discoveryLoading}
        discoveryError={discoveryError}
        githubConnected={authStatus?.signedIn ?? false}
        onRetryDiscovery={() => void pullRequestDiscoveryLifecycle.retry()}
        onConnectGitHub={() => void connectGitHub()}
        onOpenSavedReview={(review) => void openSavedReview(review)}
        onOpenReview={(review) => void navigateToReview(review)}
        onOpenPullRequest={(repository, pullRequest) => void openDiscoveredPullRequest(repository, pullRequest)}
        localBranches={<BranchReviewSelector
          refs={refs}
          currentRef={currentRef}
          defaultRef={defaultRef}
          headRef={headRef}
          baseRef={baseRef}
          action={branchSelectorAction}
          showCheckingMessage={showBranchCheckingMessage}
          opening={reviewOpeningKey === `branch:${baseRef}:${headRef}`}
          reviewNavigationBusy={Boolean(reviewOpeningKey) || refreshingReview || grantingTrust}
          reloading={branchesReloading}
          reloadError={branchReloadError}
          onHeadChange={setHeadRef}
          onBaseChange={setBaseRef}
          onReload={() => void loadRefs()}
          onSubmit={() => void (branchSelectorAction === "update" ? updateAndOpenBranchReview() : openComparison())}
        />}
        initialTab={appShell.destination.kind === "repository" ? appShell.destination.initialTab : "pull-requests"}
        openingReviewKey={reviewOpeningKey}
        reviewNavigationBusy={Boolean(reviewOpeningKey) || refreshingReview || grantingTrust}
      />
    </article>}
    <Dialog open={Boolean(pendingRepository)} onOpenChange={(open) => {
      if (!open && !grantingTrust) {
        appShell.dismissTrust();
      }
    }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("trustTitle")}</DialogTitle>
          <DialogDescription>{t("trustDescription")}</DialogDescription>
        </DialogHeader>
        <p className="break-all text-sm text-muted-foreground">{pendingRepository?.canonicalPath}</p>
        <DialogFooter>
          <Button
            variant="outline"
            disabled={grantingTrust}
            onClick={appShell.dismissTrust}
          >
            {t("cancel")}
          </Button>
          <Button aria-busy={grantingTrust || undefined} disabled={grantingTrust} onClick={() => void appShell.confirmTrust()}>
            {grantingTrust && <Spinner aria-hidden />}{t("trustAndOpen")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
    <Dialog open={githubDialogOpen} onOpenChange={setGitHubDialogOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-mono">{t("github:connect")}</DialogTitle>
          {!authStatus && <DialogDescription>{t("github:installHelp")}</DialogDescription>}
        </DialogHeader>
        {renderGitHubControls()}
      </DialogContent>
    </Dialog>
    {!catalogRuntimeEnabled && !testHarnessEnabled && <p role="alert">Narrant 需要在桌面应用中运行。</p>}
    {(testHarnessEnabled || selectedRepository) && <>
    {testHarnessEnabled && <section aria-label="打开 GitHub Pull Request">{renderGitHubControls()}</section>}
    {!selectedRepository && <><p>本地分支比较</p>
      <section aria-label="选择本地仓库" className="flex flex-wrap items-end gap-3">
        <label>本地仓库路径<Input aria-label="本地仓库路径" value={repositoryPath} onChange={(event) => setRepositoryPath(event.target.value)} /></label>
        <Button variant="outline" onClick={() => void loadRefs()}>重新读取分支</Button>
        <label>Base Ref<Select value={baseRef} onValueChange={(value) => setBaseRef(value ?? "")}><SelectTrigger aria-label="Base Ref"><SelectValue placeholder="选择 Base Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
        <label>Head Ref<Select value={headRef} onValueChange={(value) => setHeadRef(value ?? "")}><SelectTrigger aria-label="Head Ref"><SelectValue placeholder="选择 Head Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
        <Button disabled={!baseRef || !headRef || branchSelectionInvalid || Boolean(reviewOpeningKey)} aria-busy={reviewOpeningKey === `branch:${baseRef}:${headRef}` || undefined} onClick={openComparison}>
          {reviewOpeningKey === `branch:${baseRef}:${headRef}` && <Spinner aria-hidden />}
          {matchingBranchReview ? t("continueReview") : t("startReview")}
        </Button>
        {branchSelectionInvalid && <p role="status">{t("sameBranchSelection")}</p>}
      </section>
    </>}
    {visibleError && <ErrorMessage message={visibleError} />}
    {snapshot && reviewLifecycle.reader && <div className="h-[calc(100vh-4.5rem)]">
      <ReviewReader
        key={`${snapshot.reviewId}:${snapshot.snapshotId}`}
        review={{
          ...reviewLifecycle.reader,
          close: async () => {
            if (!selectedRepository) return reviewLifecycle.close();
            await appShell.navigate({ kind: "repository", repositoryEntryId: selectedRepository.id });
            return { kind: "closed" } as const;
          },
        }}
        appearance={{
          diffMode,
          narrow: narrowReview,
          editorTheme,
          initialTreeWidth,
          onDiffModeChange: selectDiffMode,
          onTreeWidthChange: (width) => localStorage.setItem(REVIEW_TREE_WIDTH_KEY, String(width)),
          onOpenExternal: (url) => void openUrl(url),
        }}
      />
    </div>}
    </>}
  </AppShell>;
}

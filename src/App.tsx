import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { AppShell } from "@/components/home-page";
import { RepositoryOverview } from "@/components/repository-overview";
import { BranchRelation } from "@/components/branch-relation";
import { BranchReviewSelector } from "@/components/branch-review-selector";
import { GitHubIdentityEntry } from "@/components/github-identity-entry";
import { GitHubAuthorizationControls } from "@/components/github-authorization-controls";
import { Spinner } from "@/components/ui/spinner";
import { RepositoryTrustDialog } from "@/components/repository-trust-dialog";
import { Toaster } from "@/components/ui/sonner";
import {
  ReviewReader,
  type DiffMode,
} from "@/components/review-reader";
import type { DiscoveredPullRequest, PullRequestGroup, RecentReviewSummary, RepositoryReviewSummary } from "@/lib/review-types";
import { createTauriReviewApplicationAdapter } from "@/infrastructure/tauri-review-application-adapter";
import { createTauriBabysitApplicationAdapter } from "@/infrastructure/tauri-babysit-application-adapter";
import { createTauriRepositoryApplicationAdapter } from "@/infrastructure/tauri-repository-application-adapter";
import { createTauriGitHubOAuthApplicationAdapter } from "@/infrastructure/tauri-github-oauth-application-adapter";
import { createTauriPullRequestDiscoveryApplicationAdapter } from "@/infrastructure/tauri-pull-request-discovery-application-adapter";
import {
  tauriTransport,
} from "@/infrastructure/tauri/transport";
import type { ReviewOpenTarget } from "@/review/review-application";
import { useGitHubIdentityLifecycle } from "@/github/use-github-identity-lifecycle";
import { usePullRequestDiscoveryLifecycle } from "@/github/use-pull-request-discovery-lifecycle";
import { useBranchSelectionContinuation } from "@/review/use-branch-selection-continuation";
import { useReviewLifecycle } from "@/review/use-review-lifecycle";
import { changeReviewEditorZoomLevel } from "@/review/editor-zoom";
import { useRepositoryOverview } from "@/repository/use-repository-overview";
import { useAppShellNavigation } from "@/app/use-app-shell-navigation";
import { SettingsPage } from "@/components/settings-page";
import { createTauriApplicationSettingsAdapter } from "@/infrastructure/tauri-application-settings-adapter";
import { useApplicationSettings } from "@/settings/use-application-settings";
import { createTauriApplicationUpdateAdapter } from "@/infrastructure/tauri-application-update-adapter";
import { useApplicationUpdate } from "@/application-update/use-application-update";
import { ApplicationUpdateAttention } from "@/components/application-update/application-update-attention";
import { ApplicationUpdateSurface } from "@/components/application-update/application-update-surface";
import "@/lib/i18n";

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
  const desktopRuntimeEnabled = "__TAURI_INTERNALS__" in window;
  const testHarnessEnabled = import.meta.env.MODE === "test";
  const applicationSettings = useMemo(() => createTauriApplicationSettingsAdapter(tauriTransport), []);
  const applicationSettingsBinding = useApplicationSettings(applicationSettings, desktopRuntimeEnabled && !testHarnessEnabled);
  const applicationUpdateApplication = useMemo(() => createTauriApplicationUpdateAdapter(tauriTransport), []);
  const applicationUpdate = useApplicationUpdate(applicationUpdateApplication, desktopRuntimeEnabled && !testHarnessEnabled);
  const editorTheme = applicationSettingsBinding.editorTheme;
  const pullRequestDiscovery = useMemo(
    () => createTauriPullRequestDiscoveryApplicationAdapter(tauriTransport),
    [],
  );
  const reviewApplicationAdapter = useMemo(
    () => createTauriReviewApplicationAdapter(tauriTransport),
    [],
  );
  const babysitApplicationAdapter = useMemo(
    () => createTauriBabysitApplicationAdapter(tauriTransport),
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
  const initialDiffMode: DiffMode = localStorage.getItem(REVIEW_DIFF_MODE_KEY) === "inline" ? "inline" : "side-by-side";
  const savedTreeWidth = Number(localStorage.getItem(REVIEW_TREE_WIDTH_KEY));
  const initialTreeWidth = Number.isFinite(savedTreeWidth) && savedTreeWidth >= 15 && savedTreeWidth <= 45
    ? savedTreeWidth
    : 24;
  const repositoryOverview = useRepositoryOverview(repositoryApplication, { enabled: desktopRuntimeEnabled });
  const reviewLifecycle = useReviewLifecycle(
    reviewApplicationAdapter,
    { initialDiffMode, initialTreeWidth },
    babysitApplicationAdapter,
  );
  const appShell = useAppShellNavigation(repositoryOverview, reviewLifecycle);
  const selectedRepository = repositoryOverview.current?.repository;
  const repositoryOverviewPresented = appShell.destination.kind === "repository"
    && appShell.destination.repositoryId === selectedRepository?.id;
  const githubIdentity = useGitHubIdentityLifecycle(githubOAuthApplication, { enabled: desktopRuntimeEnabled });
  const pullRequestDiscoveryLifecycle = usePullRequestDiscoveryLifecycle(pullRequestDiscovery, {
    enabled: repositoryOverviewPresented,
    repositoryId: selectedRepository?.id,
    signedIn: githubIdentity.status?.signedIn ?? false,
  });
  const visibleReviewReader = appShell.destination.kind === "review" ? reviewLifecycle.reader : null;
  const webviewZoomLevel = useRef(1);
  const snapshot = visibleReviewReader?.snapshot;
  const refreshingReview = visibleReviewReader?.transition.kind === "refreshing";
  const currentReaderDiffMode = reviewLifecycle.reader?.diffMode;
  const currentReaderTreeWidth = reviewLifecycle.reader?.treeWidth;
  const [repositoryPath, setRepositoryPath] = useState("");
  const [baseRef, setBaseRef] = useState("");
  const [headRef, setHeadRef] = useState("");
  const [error, setError] = useState("");
  const [githubDialogOpen, setGitHubDialogOpen] = useState(false);
  const [narrowReview, setNarrowReview] = useState(() => window.innerWidth < 1200);
  const repositoryEntries = repositoryOverview.navigation.repositories;
  const recentReviews = repositoryOverview.navigation.recentReviews;
  const repositoryReviews = repositoryOverview.current?.reviews ?? [];
  const pendingRepository = appShell.trust?.repository;
  const grantingTrust = appShell.trust?.granting ?? false;
  const navigationLoading = repositoryOverview.navigation.status === "loading"
    || repositoryOverview.transition.kind === "registering";
  const branchState = repositoryOverview.branches;
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
  const navigateAppShell = appShell.navigate;
  /** 将 continuation 的稳定决策交给 App Shell Navigation 执行。 */
  const openBranchSelection = useCallback(
    (target: Extract<ReviewOpenTarget, { kind: "branch-selection" }>) => navigateAppShell({
      kind: "review",
      repositoryId: target.repositoryId,
      target,
    }),
    [navigateAppShell],
  );
  const branchContinuation = useBranchSelectionContinuation(reviewApplicationAdapter, {
    repositoryId: snapshot
      ? undefined
      : repositoryOverviewPresented
        ? selectedRepository?.id
        : testHarnessEnabled ? 0 : undefined,
    baseRef,
    headRef,
    reviews: repositoryReviews,
    openReview: openBranchSelection,
  });

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

  useEffect(() => {
    /** 文本 Reader 优先接管快捷键；其余 Reader 保存字号，其他页面缩放 WebView。 */
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || (!event.ctrlKey && !event.metaKey) || event.altKey) return;
      if (event.code === "KeyP") {
        event.preventDefault();
        return;
      }
      const direction: -1 | 0 | 1 = event.code === "Equal" || event.code === "NumpadAdd"
        ? 1
        : event.code === "Minus" || event.code === "NumpadSubtract"
          ? -1
          : 0;
      if (direction === 0 && event.code !== "Digit0" && event.code !== "Numpad0") return;
      event.preventDefault();
      if (visibleReviewReader) {
        changeReviewEditorZoomLevel(direction);
        return;
      }
      if (!desktopRuntimeEnabled) return;
      const nextLevel = direction === 0
        ? 1
        : Math.min(10, Math.max(0.2, Math.round((webviewZoomLevel.current + direction * 0.2) * 10) / 10));
      webviewZoomLevel.current = nextLevel;
      void getCurrentWebview().setZoom(nextLevel);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [desktopRuntimeEnabled, visibleReviewReader]);

  useEffect(() => {
    if (!currentReaderDiffMode || currentReaderTreeWidth === undefined) return;
    localStorage.setItem(REVIEW_DIFF_MODE_KEY, currentReaderDiffMode);
    localStorage.setItem(REVIEW_TREE_WIDTH_KEY, String(currentReaderTreeWidth));
  }, [currentReaderDiffMode, currentReaderTreeWidth]);

  useEffect(() => {
    if (branchState.kind !== "ready") return;
    setBaseRef(branchState.value.defaultRef);
    setHeadRef(branchState.value.currentRef);
  }, [branchState]);

  /** 验证并持久化用户输入的具体本地 clone。 */
  async function registerRepository() {
    setError("");
    try {
      await repositoryOverview.register(repositoryPath);
    } catch (reason) {
      setError(String(reason));
    }
  }

  /** 原生目录选择后的 Repository 注册入口。 */
  async function registerRepositoryFolder(path: string) {
    setRepositoryPath(path);
    setError("");
    const result = await appShell.registerAndOpen(path);
    if (result.kind === "failed") setError(repositoryOpenError(result.detail));
    return result;
  }

  /** Repository Overview 提交后启动独立的页面数据读取。 */
  useEffect(() => {
    if (!selectedRepository || !repositoryOverviewPresented) return;
    setRepositoryPath(selectedRepository.canonicalPath);
    if (!testHarnessEnabled) void repositoryOverview.reloadBranches();
    // 页面数据只在已提交的 Repository identity 改变后重新读取。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [repositoryOverviewPresented, selectedRepository?.id]);

  /** 不依赖 GitHub 登录或 discovery，从 cached current Snapshot 重开 Review。 */
  async function openSavedReview(review: RepositoryReviewSummary) {
    if (!selectedRepository) return;
    setError("");
    const result = await appShell.navigate({
      kind: "review",
      repositoryId: selectedRepository.id,
      target: { kind: "saved-review", reviewId: review.reviewId },
    });
    if (result.kind === "failed") setError(result.detail);
  }

  /** 侧边栏 Review 导航统一处理跨仓库、Trust 与失败回滚。 */
  async function navigateToReview(review: RecentReviewSummary) {
    setError("");
    const result = await appShell.navigate({
      kind: "review",
      repositoryId: review.repositoryId,
      target: { kind: "saved-review", reviewId: review.review.reviewId },
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
      repositoryId: selectedRepository.id,
      target: {
        kind: "pull-request-selection",
        repositoryId: selectedRepository.id,
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
  async function loadRefs(repositoryId = selectedRepository?.id) {
    const resolvedRepositoryId = repositoryId ?? (testHarnessEnabled ? 0 : undefined);
    if (resolvedRepositoryId === undefined) return;
    try {
      await repositoryOverview.reloadBranches(resolvedRepositoryId);
    } catch (reason) {
      setError(String(reason));
    }
  }

  useEffect(() => {
    if (!import.meta.env.VITE_WDIO) return;
    const target = window as Window & {
      __NARRANT_E2E_RELOAD_REPOSITORY_NAVIGATION__?: () => Promise<void>;
    };
    target.__NARRANT_E2E_RELOAD_REPOSITORY_NAVIGATION__ = repositoryOverview.reloadNavigation;
    return () => {
      delete target.__NARRANT_E2E_RELOAD_REPOSITORY_NAVIGATION__;
    };
  }, [repositoryOverview.reloadNavigation]);

  /** 统一将完整 object id 压缩为用户可辨认的短 revision。 */
  return <>
  <AppShell
    navigation={{
      ...appShell,
      registerAndOpen: registerRepositoryFolder,
      sidebar: {
        ...appShell.sidebar,
        error: !selectedRepository ? visibleError || undefined : undefined,
      },
    }}
    enabled={desktopRuntimeEnabled && (!testHarnessEnabled || Boolean(selectedRepository))}
    fallbackClassName="narrant-app"
    githubIdentityEntry={renderGitHubIdentityEntry()}
    applicationUpdateAttention={<ApplicationUpdateAttention snapshot={applicationUpdate.snapshot} onOpen={applicationUpdate.openSurface} />}
  >
    <Toaster position="top-right" />
    {(!desktopRuntimeEnabled || testHarnessEnabled) && <header><h1>{t("brand")}</h1><p>{t("subtitle")}</p></header>}
    {!testHarnessEnabled && !desktopRuntimeEnabled && <div className="fixed bottom-4 left-4 z-40 w-56 rounded-xl border bg-background/95 p-2 shadow-lg backdrop-blur">
      {renderGitHubIdentityEntry()}
    </div>}
    {desktopRuntimeEnabled && !selectedRepository && testHarnessEnabled && <article aria-label="Persistence">
      <h2>Repositories</h2>
      <section aria-label="添加 Repository" className="flex flex-wrap items-end gap-3">
        <label>Repository 路径<Input aria-label="Repository 路径" value={repositoryPath} onChange={(event) => setRepositoryPath(event.target.value)} /></label>
        <Button disabled={!repositoryPath || navigationLoading} onClick={registerRepository}>
          {navigationLoading ? "正在读取…" : "添加 Repository"}
        </Button>
      </section>
      {repositoryEntries.length === 0
        ? <p>{navigationLoading ? "正在加载 Repositories…" : "尚未添加 Repository"}</p>
        : <ul aria-label="Repositories">
          {repositoryEntries.map((entry) => <li key={entry.id}>
            <Button variant="outline" disabled={entry.availability.kind !== "available"} onClick={() => void appShell.navigate({ kind: "repository", repositoryId: entry.id })}>
              {entry.canonicalPath}
            </Button>
            {entry.availability.kind !== "available" && <>
              <strong>不可用</strong>
              <p>{entry.availability.kind === "unavailable" ? entry.availability.reason : undefined}</p>
            </>}
          </li>)}
        </ul>}
      <section aria-label="Recent Reviews">
        <h3>Recent Reviews</h3>
        {recentReviews.length === 0
          ? <p>尚无最近 Review</p>
          : <ul>{recentReviews.map((recent) => <li key={recent.review.reviewId}>
            <Button variant="outline" disabled={Boolean(reviewOpeningKey)} aria-busy={reviewOpeningKey === `review:${recent.review.reviewId}` || undefined} onClick={() => void navigateToReview(recent)}>
              {reviewOpeningKey === `review:${recent.review.reviewId}` && <Spinner aria-hidden />}
              <span className="mr-3">{recent.repositoryPath}</span><BranchRelation baseRef={recent.review.selection.baseRef} headRef={recent.review.selection.headRef} interactive={false} />
            </Button>
          </li>)}</ul>}
      </section>
    </article>}
    {selectedRepository && repositoryOverviewPresented && !snapshot && <article aria-label="Repository">
      <RepositoryOverview
        repositoryName={selectedRepository.canonicalPath.split(/[\\/]/).filter(Boolean).at(-1) ?? selectedRepository.canonicalPath}
        repositoryPath={selectedRepository.canonicalPath}
        reviews={repositoryReviews}
        pullRequestGroups={pullRequestGroups}
        discoveryLoading={discoveryLoading}
        discoveryError={discoveryError}
        githubConnected={authStatus?.signedIn ?? false}
        onRetryDiscovery={() => void pullRequestDiscoveryLifecycle.retry()}
        onConnectGitHub={() => void connectGitHub()}
        onOpenReview={(review) => void openSavedReview(review)}
        onOpenPullRequest={(repository, pullRequest) => void openDiscoveredPullRequest(repository, pullRequest)}
        localBranches={<BranchReviewSelector
          refs={refs}
          currentRef={currentRef}
          defaultRef={defaultRef}
          headRef={headRef}
          baseRef={baseRef}
          continuation={branchContinuation}
          opening={reviewOpeningKey === `branch:${baseRef}:${headRef}`}
          reviewNavigationBusy={Boolean(reviewOpeningKey) || refreshingReview || grantingTrust}
          reloading={branchesReloading}
          reloadError={branchReloadError}
          onHeadChange={setHeadRef}
          onBaseChange={setBaseRef}
          onReload={() => void loadRefs()}
        />}
        initialTab={appShell.destination.kind === "repository" ? appShell.destination.initialTab : "pull-requests"}
        openingReviewKey={reviewOpeningKey}
        reviewNavigationBusy={Boolean(reviewOpeningKey) || refreshingReview || grantingTrust}
      />
    </article>}
    {appShell.destination.kind === "settings" && <SettingsPage
      settings={applicationSettingsBinding}
      application={applicationSettings}
      githubIdentity={githubIdentity}
      applicationUpdate={applicationUpdate}
      initialSection={appShell.destination.initialSection}
      onBack={() => void appShell.returnFromSettings()}
    />}
    <ApplicationUpdateSurface update={applicationUpdate} />
    <Dialog open={githubDialogOpen} onOpenChange={setGitHubDialogOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="font-mono">{t("github:connect")}</DialogTitle>
          {!authStatus && <DialogDescription>{t("github:installHelp")}</DialogDescription>}
        </DialogHeader>
        {renderGitHubControls()}
      </DialogContent>
    </Dialog>
    {!desktopRuntimeEnabled && !testHarnessEnabled && <p role="alert">Narrant 需要在桌面应用中运行。</p>}
    {(testHarnessEnabled || selectedRepository) && <>
    {testHarnessEnabled && <section aria-label="打开 GitHub Pull Request">{renderGitHubControls()}</section>}
    {!selectedRepository && <><p>本地分支比较</p>
      <section aria-label="选择本地仓库" className="flex flex-wrap items-end gap-3">
        <label>本地仓库路径<Input aria-label="本地仓库路径" value={repositoryPath} onChange={(event) => setRepositoryPath(event.target.value)} /></label>
        <Button variant="outline" onClick={() => void loadRefs()}>重新读取分支</Button>
        <label>Base Ref<Select value={baseRef} onValueChange={(value) => setBaseRef(value ?? "")}><SelectTrigger aria-label="Base Ref"><SelectValue placeholder="选择 Base Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
        <label>Head Ref<Select value={headRef} onValueChange={(value) => setHeadRef(value ?? "")}><SelectTrigger aria-label="Head Ref"><SelectValue placeholder="选择 Head Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
        <Button
          disabled={!baseRef || !headRef || branchContinuation.state.kind === "invalid" || branchContinuation.state.kind === "checking" || Boolean(reviewOpeningKey)}
          aria-busy={reviewOpeningKey === `branch:${baseRef}:${headRef}` || undefined}
          onClick={() => void branchContinuation.submit()}
        >
          {reviewOpeningKey === `branch:${baseRef}:${headRef}` && <Spinner aria-hidden />}
          {branchContinuation.state.kind === "ready" && branchContinuation.state.action !== "start" ? t("continueReview") : t("startReview")}
        </Button>
        {branchContinuation.state.kind === "invalid" && <p role="status">{t("sameBranchSelection")}</p>}
      </section>
    </>}
    {visibleError && <ErrorMessage message={visibleError} />}
    {snapshot && visibleReviewReader && <div className="h-[calc(100vh-4.5rem)]">
      <ReviewReader
        key={snapshot.reviewId}
        review={{
          ...visibleReviewReader,
          close: async () => {
            if (!selectedRepository) return reviewLifecycle.close();
            await appShell.navigate({ kind: "repository", repositoryId: selectedRepository.id });
            return { kind: "closed" } as const;
          },
        }}
        appearance={{
          narrow: narrowReview,
          editorTheme,
          onOpenExternal: (url) => void openUrl(url),
          onOpenApplicationSettings: () => void appShell.navigate({ kind: "settings", initialSection: "babysit" }),
        }}
      />
    </div>}
    </>}
  </AppShell>
  {pendingRepository && <RepositoryTrustDialog
    open
    canonicalPath={pendingRepository.canonicalPath}
    granting={grantingTrust}
    onCancel={appShell.dismissTrust}
    onConfirm={() => void appShell.confirmTrust()}
  />}
  </>;
}

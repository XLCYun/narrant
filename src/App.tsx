import { useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Copy01Icon } from "@hugeicons/core-free-icons";
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
import { Spinner } from "@/components/ui/spinner";
import { Toaster } from "@/components/ui/sonner";
import { Icon } from "@/components/ui/icon";
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
import type { DeviceAuthorization, GitHubAuthStatus } from "@/github/github-oauth-application";
import type { RepositoryEntry } from "@/repository/repository-application";
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
  const snapshot = reviewLifecycle.reader?.snapshot;
  const refreshingReview = reviewLifecycle.reader?.transition.kind === "refreshing";
  const [repositoryPath, setRepositoryPath] = useState("");
  const [pullRequestGroups, setPullRequestGroups] = useState<PullRequestGroup[]>([]);
  const [discoveryLoading, setDiscoveryLoading] = useState(false);
  const [discoveryError, setDiscoveryError] = useState<string>();
  const [baseRef, setBaseRef] = useState("");
  const [headRef, setHeadRef] = useState("");
  const [branchSelectionCheck, setBranchSelectionCheck] = useState<BranchSelectionCheck>({ status: "idle" });
  const [showBranchCheckingMessage, setShowBranchCheckingMessage] = useState(false);
  const branchSelectionCheckId = useRef(0);
  const [error, setError] = useState("");
  const [authStatus, setAuthStatus] = useState<GitHubAuthStatus>();
  const [githubDialogOpen, setGitHubDialogOpen] = useState(false);
  const [deviceAuthorization, setDeviceAuthorization] = useState<DeviceAuthorization>();
  const [authorizing, setAuthorizing] = useState(false);
  const [copiedValue, setCopiedValue] = useState<"device-code" | "authorization-url">();
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
  const selectedRepository = repositoryContext.current?.repository;
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

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    void githubOAuthApplication.loadStatus()
      .then(setAuthStatus)
      .catch(() => setAuthStatus(undefined));
  }, [githubOAuthApplication]);

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

  /** 使用当前匿名或已刷新凭据重新加载各 GitHub repository 的独立结果。 */
  async function loadPullRequestGroups(entry: RepositoryEntry) {
    setDiscoveryError(undefined);
    setDiscoveryLoading(true);
    try {
      setPullRequestGroups(await pullRequestDiscovery.discover(entry.id));
    } catch (reason) {
      setDiscoveryError(String(reason));
    } finally {
      setDiscoveryLoading(false);
    }
  }

  /** Repository context 提交后启动独立的页面数据读取。 */
  useEffect(() => {
    if (!selectedRepository) return;
    setRepositoryPath(selectedRepository.canonicalPath);
    setPullRequestGroups([]);
    setDiscoveryError(undefined);
    void loadPullRequestGroups(selectedRepository);
    if (!testHarnessEnabled) void repositoryContext.reloadBranches();
    void Promise.resolve(githubOAuthApplication.loadStatus())
      .then(setAuthStatus)
      .catch(() => setAuthStatus(undefined));
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
    setCopiedValue(undefined);
    if (!testHarnessEnabled) setGitHubDialogOpen(true);
    try {
      const status = await githubOAuthApplication.loadStatus();
      setAuthStatus(status);
      if (status.configured && !status.signedIn && status.appInstallUrl) {
        await openUrl(status.appInstallUrl);
      }
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function startGitHubAuthorization() {
    setError("");
    setCopiedValue(undefined);
    setAuthorizing(true);
    try {
      const authorization = await githubOAuthApplication.beginAuthorization();
      setDeviceAuthorization(authorization);
      await openUrl(authorization.verificationUri);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setAuthorizing(false);
    }
  }

  /** 将用户主动选择的授权信息复制到系统剪贴板，并反馈复制结果。 */
  async function copyAuthorizationValue(value: string, copied: "device-code" | "authorization-url") {
    try {
      await navigator.clipboard.writeText(value);
      setCopiedValue(copied);
    } catch (reason) {
      toast.error(t("transientError"), { description: String(reason) });
    }
  }

  async function completeGitHubAuthorization() {
    if (!deviceAuthorization) return;
    setError("");
    setAuthorizing(true);
    try {
      const status = await githubOAuthApplication.completeAuthorization(deviceAuthorization.authorizationId);
      setAuthStatus(status);
      setDeviceAuthorization(undefined);
      setGitHubDialogOpen(false);
      if (selectedRepository) await loadPullRequestGroups(selectedRepository);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setAuthorizing(false);
    }
  }

  async function signOutGitHub() {
    setError("");
    const result = await githubOAuthApplication.signOut();
    if (result.kind === "signed-out") {
      setAuthStatus(result.status);
      setDeviceAuthorization(undefined);
      if (selectedRepository) await loadPullRequestGroups(selectedRepository);
      return;
    }
    if (result.status) setAuthStatus(result.status);
    setError(result.detail);
  }

  /** 按安装与账号授权两个独立步骤渲染 GitHub 连接向导。 */
  function renderGitHubControls() {
    return <div className="min-w-0 space-y-3 font-mono text-sm">
      {!authStatus && <Button variant="ghost" className="w-full justify-start" onClick={connectGitHub}>{t("github:connect")}</Button>}
      {authStatus?.signedIn && <Button variant="ghost" className="w-full justify-start" onClick={signOutGitHub}>{t("github:signOut")}</Button>}
      {authStatus && !authStatus.signedIn && (!authStatus.configured
        ? <p className="px-2 text-xs text-muted-foreground">{t("github:unconfigured")}</p>
        : deviceAuthorization
          ? <div className="min-w-0 space-y-5 py-2">
            <p className="text-center leading-6">
              {t("github:authorizationPromptBefore")}{" "}
              <span>
                <a
                  className="break-all underline underline-offset-4 hover:text-foreground"
                  href={deviceAuthorization.verificationUri}
                  onClick={(event) => {
                    event.preventDefault();
                    void openUrl(deviceAuthorization.verificationUri);
                  }}
                >
                  {deviceAuthorization.verificationUri}
                </a>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  className="ml-1 inline-flex cursor-pointer align-middle"
                  aria-label={copiedValue === "authorization-url" ? t("github:linkCopied") : t("github:copyLink")}
                  onClick={() => void copyAuthorizationValue(deviceAuthorization.verificationUri, "authorization-url")}
                >
                  <Icon icon={Copy01Icon} />
                </Button>
              </span>{" "}
              {t("github:authorizationPromptAfter")}
            </p>
            <div
              aria-label={`${t("github:deviceCode")} ${deviceAuthorization.userCode}`}
              className="grid w-full grid-cols-[repeat(4,minmax(0,1fr))_auto_repeat(4,minmax(0,1fr))] items-center gap-1.5 sm:gap-2"
            >
              {Array.from(deviceAuthorization.userCode).map((character, index) => (
                character === "-"
                  ? <span key={`${character}-${index}`} aria-hidden className="px-0.5 text-center text-2xl font-bold">-</span>
                  : <span
                    key={`${character}-${index}`}
                    aria-hidden
                    className="flex aspect-square min-w-0 items-center justify-center rounded-md border bg-input/20 text-xl font-bold"
                  >
                    {character}
                  </span>
              ))}
            </div>
            <Button variant="outline" className="w-full" onClick={() => void copyAuthorizationValue(deviceAuthorization.userCode, "device-code")}>
              {copiedValue === "device-code" ? t("github:deviceCodeCopied") : t("github:copyDeviceCode")}
            </Button>
            <Button className="w-full" disabled={authorizing} onClick={completeGitHubAuthorization}>{t("github:authorizationComplete")}</Button>
          </div>
          : <div className="space-y-6">
          {authStatus.appInstallUrl && <div className="space-y-3 text-center">
            <p className="leading-6">
              <a
                className="break-all text-foreground underline underline-offset-4"
                href={authStatus.appInstallUrl}
                onClick={(event) => {
                  event.preventDefault();
                  void openUrl(authStatus.appInstallUrl!);
                }}
              >
                {authStatus.appInstallUrl}
              </a>
              <Button
                variant="ghost"
                size="icon-xs"
                className="ml-1 inline-flex cursor-pointer align-middle"
                aria-label={copiedValue === "authorization-url" ? t("github:linkCopied") : t("github:copyLink")}
                onClick={() => void copyAuthorizationValue(authStatus.appInstallUrl!, "authorization-url")}
              >
                <Icon icon={Copy01Icon} />
              </Button>
            </p>
            <p className="leading-6 text-muted-foreground">{t("github:installHelp")}</p>
          </div>}
          <Button className="w-full" disabled={authorizing} onClick={startGitHubAuthorization}>{t("github:installedContinue")}</Button>
        </div>)}
    </div>;
  }

  /** 在主侧栏和旧布局中复用同一个 GitHub Identity 全局入口。 */
  function renderGitHubIdentityEntry(className = "h-auto min-h-12 w-full justify-start py-2") {
    return <GitHubIdentityEntry
      className={className}
      signedIn={authStatus?.signedIn ?? false}
      identity={authStatus?.identity}
      onConnect={() => void connectGitHub()}
      onSignOut={() => void signOutGitHub()}
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
        onRetryDiscovery={() => void loadPullRequestGroups(selectedRepository)}
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

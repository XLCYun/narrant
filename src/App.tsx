import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Copy01Icon } from "@hugeicons/core-free-icons";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useSystemTheme } from "@/lib/use-system-theme";
import { AppShell, type HomeRepository } from "@/components/home-page";
import { RepositoryOverview } from "@/components/repository-overview";
import { BranchRelation } from "@/components/branch-relation";
import { BranchReviewSelector, type BranchSelectorAction } from "@/components/branch-review-selector";
import { GitHubIdentityEntry, type GitHubIdentity } from "@/components/github-identity-entry";
import { Spinner } from "@/components/ui/spinner";
import { Toaster } from "@/components/ui/sonner";
import { Icon } from "@/components/ui/icon";
import {
  ReviewReader,
  type DiffMode,
  type FileChangeContent,
  type ReviewReaderSnapshot,
} from "@/components/review-reader";
import type { DiscoveredPullRequest, PullRequestGroup, ReviewSummary, SavedReviewSummary } from "@/lib/review-types";
import "@/lib/i18n";

type ReviewSnapshot = ReviewReaderSnapshot;
type BranchReviewUpdate = {
  changed: boolean;
  currentBaseRevision: string;
  currentHeadRevision: string;
  latestBaseRevision: string;
  latestHeadRevision: string;
};
/** Repository Entry 中已有 Branch Selection 的更新检查状态。 */
type BranchSelectionCheck =
  | { status: "idle" | "checking" | "current" }
  | { status: "update"; update: BranchReviewUpdate }
  | { status: "failed" };
/** 应用边界显式返回的分支选项与 Repository Entry 默认选择。 */
type BranchRefs = {
  refs: string[];
  defaultRef: string;
  currentRef: string;
};
type ActiveReview = { reviewId: string };
/** 侧边栏中正在打开的单一导航目标。 */
type PendingNavigation = { type: "repository"; id: number } | { type: "review"; id: string };
type GitHubAuthStatus = {
  configured: boolean;
  signedIn: boolean;
  appInstallUrl: string | null;
  identity: GitHubIdentity | null;
};
type DeviceAuthorization = {
  authorizationId: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
};
type RepositoryEntry = {
  id: number;
  canonicalPath: string;
  commonDirectory: string;
  createdAt: number;
  lastOpenedAt: number;
  available: boolean;
  unavailableReason: string | null;
};
/// 当前由 discovery 选中的 Pull Request identity 与 metadata。
type SelectedPullRequest = {
  repository: PullRequestGroup["repository"];
  pullRequest: DiscoveredPullRequest;
};
/// “检查更新”返回的 current 与 available revision 对照。
type ReviewUpdate = {
  updateAvailable: boolean;
  currentBaseRevision: string;
  currentHeadRevision: string;
  availableBaseRevision: string;
  availableHeadRevision: string;
};


const REVIEW_TREE_WIDTH_KEY = "narrant.review.fileTreeWidth";
const REVIEW_DIFF_MODE_KEY = "narrant.review.diffMode";

/** 以不依赖系统 locale 的代码点顺序稳定排列路径。 */
function comparePaths(left: string, right: string) {
  return left < right ? -1 : left > right ? 1 : 0;
}

/** 更新已有 Repository Entry 时保持位置，只把新添加的仓库放到列表前端。 */
function upsertRepositoryEntry(entries: RepositoryEntry[], entry: RepositoryEntry) {
  return entries.some((candidate) => candidate.id === entry.id)
    ? entries.map((candidate) => candidate.id === entry.id ? entry : candidate)
    : [entry, ...entries];
}

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
  const [repositoryPath, setRepositoryPath] = useState("");
  const [repositoryEntries, setRepositoryEntries] = useState<RepositoryEntry[]>([]);
  const [recentReviews, setRecentReviews] = useState<ReviewSummary[]>([]);
  const [repositoryReviews, setRepositoryReviews] = useState<ReviewSummary[]>([]);
  const [selectedRepository, setSelectedRepository] = useState<RepositoryEntry>();
  const [pendingNavigation, setPendingNavigation] = useState<PendingNavigation>();
  const [pendingRepository, setPendingRepository] = useState<RepositoryEntry>();
  const [pendingReview, setPendingReview] = useState<ReviewSummary>();
  const [reviewOpeningKey, setReviewOpeningKey] = useState<string>();
  const [grantingTrust, setGrantingTrust] = useState(false);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [pullRequestGroups, setPullRequestGroups] = useState<PullRequestGroup[]>([]);
  const [savedReviews, setSavedReviews] = useState<SavedReviewSummary[]>([]);
  const [discoveryLoading, setDiscoveryLoading] = useState(false);
  const [discoveryError, setDiscoveryError] = useState<string>();
  const [refs, setRefs] = useState<string[]>([]);
  const [defaultRef, setDefaultRef] = useState("");
  const [currentRef, setCurrentRef] = useState("");
  const [baseRef, setBaseRef] = useState("");
  const [headRef, setHeadRef] = useState("");
  const [branchSelectionCheck, setBranchSelectionCheck] = useState<BranchSelectionCheck>({ status: "idle" });
  const [showBranchCheckingMessage, setShowBranchCheckingMessage] = useState(false);
  const [branchesReloading, setBranchesReloading] = useState(false);
  const [branchReloadError, setBranchReloadError] = useState("");
  const branchSelectionCheckId = useRef(0);
  const [snapshot, setSnapshot] = useState<ReviewSnapshot>();
  const [selectedPullRequest, setSelectedPullRequest] = useState<SelectedPullRequest>();
  const [reviewUpdate, setReviewUpdate] = useState<ReviewUpdate>();
  const [branchUpdate, setBranchUpdate] = useState<BranchReviewUpdate>();
  const [reviewUpdateError, setReviewUpdateError] = useState("");
  const [refreshingReview, setRefreshingReview] = useState(false);
  const [repositoryInitialTab, setRepositoryInitialTab] = useState<"pull-requests" | "branches" | "history">("pull-requests");
  const [fileChange, setFileChange] = useState<FileChangeContent>();
  const [fileChangeLoading, setFileChangeLoading] = useState(false);
  const [fileChangeError, setFileChangeError] = useState("");
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
  const activeReview = useRef<ActiveReview>();
  const activeSnapshotId = useRef<string>();
  const activeGitOperationId = useRef<string>();
  const nextGitOperationId = useRef(0);
  const openFileChangeRef = useRef<(path: string) => Promise<void>>();
  const fileChangeRequestId = useRef(0);
  const [pendingFilePath, setPendingFilePath] = useState<string>();

  /** 为 Pull Request 入口生成与 Repository Overview 行一致的稳定 pending identity。 */
  function pullRequestReviewKey(owner: string, repository: string, number: number) {
    return `${owner.toLowerCase()}/${repository.toLowerCase()}#${number}`;
  }

  /** 等待 busy 状态完成一次实际绘制，再启动可能阻塞界面线程的后端操作。 */
  function waitForReviewOpeningPaint() {
    return new Promise<void>((resolve) => {
      window.requestAnimationFrame(() => window.requestAnimationFrame(() => resolve()));
    });
  }

  /** 串行化 Review Reader 导航，并只清理当前操作拥有的 busy 状态。 */
  async function withReviewOpening<T>(key: string, action: () => Promise<T>): Promise<T | undefined> {
    if (reviewOpeningKey || refreshingReview || grantingTrust) return undefined;
    setReviewOpeningKey(key);
    try {
      await waitForReviewOpeningPaint();
      return await action();
    } finally {
      setReviewOpeningKey((current) => current === key ? undefined : current);
    }
  }

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

  /** 执行可取消的本地 Core Git 命令，并在新操作开始时取消旧操作。 */
  async function invokeLocalGit<T>(command: string, arguments_: Record<string, unknown>) {
    const previousOperationId = activeGitOperationId.current;
    if (previousOperationId) {
      void Promise.resolve(invoke("cancel_git_operation", { operationId: previousOperationId }))
        .catch(() => undefined);
    }
    const operationId = `local-git-${nextGitOperationId.current++}`;
    activeGitOperationId.current = operationId;
    try {
      return await invoke<T>(command, { ...arguments_, operationId });
    } finally {
      if (activeGitOperationId.current === operationId) activeGitOperationId.current = undefined;
    }
  }

  /** 后台检查使用独立 operation identity，不取消用户正在读取的 File Change。 */
  async function invokeBackgroundGit<T>(command: string, arguments_: Record<string, unknown>) {
    const operationId = `background-git-${nextGitOperationId.current++}`;
    return invoke<T>(command, { ...arguments_, operationId });
  }

  function replaceSnapshot(nextSnapshot?: ReviewSnapshot) {
    fileChangeRequestId.current += 1;
    setFileChangeLoading(false);
    setPendingFilePath(undefined);
    const previousReview = activeReview.current;
    const nextReview = nextSnapshot && { reviewId: nextSnapshot.reviewId };
    activeReview.current = nextReview;
    activeSnapshotId.current = nextSnapshot?.snapshotId;
    setSnapshot(nextSnapshot);
    setBranchUpdate(undefined);
    if (previousReview && JSON.stringify(previousReview) !== JSON.stringify(nextReview)) {
      closeActiveReview(previousReview);
    }
  }

  /** 使用 durable reviewId 关闭当前运行时资源。 */
  function closeActiveReview(review: ActiveReview) {
    void Promise.resolve(invoke("close_review", { reviewId: review.reviewId }))
      .catch(() => undefined);
  }

  /** 关闭当前 Review 的运行时资源并保留持久化历史。 */
  function closeReviewView(initialTab: "pull-requests" | "branches" | "history" = "history") {
    replaceSnapshot(undefined);
    setFileChange(undefined);
    setReviewUpdate(undefined);
    setReviewUpdateError("");
    setRepositoryInitialTab(initialTab);
  }

  /** 返回 Catalog 时同时关闭当前 Review，避免页面外仍保留 workspace。 */
  function leaveRepository() {
    closeReviewView();
    setSelectedRepository(undefined);
    setRepositoryReviews([]);
  }

  useEffect(() => () => {
    if (activeGitOperationId.current) {
      void Promise.resolve(invoke("cancel_git_operation", { operationId: activeGitOperationId.current }))
        .catch(() => undefined);
      activeGitOperationId.current = undefined;
    }
    if (activeReview.current) {
      closeActiveReview(activeReview.current);
      activeReview.current = undefined;
    }
  }, []);

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
    if (!("__TAURI_INTERNALS__" in window)) return;
    setCatalogLoading(true);
    void Promise.all([
      invoke<RepositoryEntry[]>("list_repository_entries"),
      invoke<ReviewSummary[]>("list_reviews", { repositoryEntryId: null }),
    ])
      .then(([entries, reviews]) => {
        setRepositoryEntries(entries);
        setRecentReviews(reviews ?? []);
      })
      .catch((reason) => setError(String(reason)))
      .finally(() => setCatalogLoading(false));
  }, []);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    void invoke<GitHubAuthStatus>("github_auth_status")
      .then((status) => {
        setAuthStatus(status);
        if (!status.signedIn) return;
        void invoke<GitHubIdentity>("refresh_github_identity")
          .then((identity) => setAuthStatus((current) => current ? { ...current, identity } : current))
          .catch(() => undefined);
      })
      .catch(() => setAuthStatus(undefined));
  }, []);

  /** 验证并持久化用户输入的具体本地 clone。 */
  async function registerRepository() {
    setError("");
    setCatalogLoading(true);
    try {
      const entry = await invoke<RepositoryEntry>("register_repository_entry", {
        path: repositoryPath,
      });
      setRepositoryEntries((entries) => upsertRepositoryEntry(entries, entry));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setCatalogLoading(false);
    }
  }

  /** 原生目录选择后的 Repository Entry 注册入口。 */
  async function registerRepositoryFolder(path: string) {
    setRepositoryPath(path);
    setError("");
    try {
      const entry = await invoke<RepositoryEntry>("register_repository_entry", { path });
      setRepositoryEntries((entries) => upsertRepositoryEntry(entries, entry));
      await openRepository(entry);
    } catch (reason) { setError(repositoryOpenError(reason)); }
  }

  /** 使用当前匿名或已刷新凭据重新加载各 GitHub repository 的独立结果。 */
  async function loadPullRequestGroups(entry: RepositoryEntry) {
    setDiscoveryError(undefined);
    setDiscoveryLoading(true);
    try {
      setPullRequestGroups(await invoke<PullRequestGroup[]>("discover_github_pull_requests", {
        repositoryEntryId: entry.id,
      }));
    } catch (reason) {
      setDiscoveryError(String(reason));
    } finally {
      setDiscoveryLoading(false);
    }
  }

  /** 从持久化 Catalog 重新加载 Repository Entry 的 Review 历史。 */
  async function loadSavedReviews(repositoryEntryId: number) {
    const reviews = await invoke<SavedReviewSummary[]>("list_saved_reviews", {
      repositoryEntryId,
    });
    setSavedReviews(reviews ?? []);
  }

  /** 在 Workspace Trust 已确认后进入 Repository Entry 上下文。 */
  async function enterRepository(opened: RepositoryEntry) {
    const [saved, reviews] = await Promise.all([
      invoke<SavedReviewSummary[]>("list_saved_reviews", { repositoryEntryId: opened.id }),
      invoke<ReviewSummary[]>("list_reviews", { repositoryEntryId: opened.id }),
    ]);
    setSelectedRepository(opened);
    setSavedReviews(saved ?? []);
    setRepositoryReviews(reviews ?? []);
    setRepositoryPath(opened.canonicalPath);
    setPullRequestGroups([]);
    setDiscoveryError(undefined);
    void loadPullRequestGroups(opened);
    if (!testHarnessEnabled) void loadRefs(opened.id);
    void Promise.resolve(invoke<GitHubAuthStatus>("github_auth_status"))
      .then(setAuthStatus)
      .catch(() => setAuthStatus(undefined));
    if (!snapshot) setFileChange(undefined);
  }

  /** 打开 Repository Entry；首次使用时先停留当前页面并请求 Workspace Trust。 */
  async function openRepository(entry: RepositoryEntry): Promise<"entered" | "trust-required" | "failed"> {
    setError("");
    const previousRepositoryId = selectedRepository?.id;
    const leavingReview = Boolean(snapshot);
    try {
      const opened = await invoke<RepositoryEntry>("open_repository_entry", { id: entry.id });
      setRepositoryEntries((entries) => upsertRepositoryEntry(entries, opened));
      if (!opened.available) {
        setError(repositoryOpenError(opened.unavailableReason ?? "Repository Entry 当前不可用。"));
        return "failed";
      }
      const trusted = await invoke<boolean>("repository_workspace_trust", {
        repositoryEntryId: opened.id,
      });
      if (!trusted) {
        setPendingRepository(opened);
        return "trust-required";
      }
      await enterRepository(opened);
      if (leavingReview) {
        closeReviewView(previousRepositoryId === opened.id ? "history" : "pull-requests");
      } else {
        setRepositoryInitialTab("pull-requests");
      }
      return "entered";
    } catch (reason) {
      setError(String(reason));
      return "failed";
    }
  }

  /** 不依赖 GitHub 登录或 discovery，从 cached current Snapshot 重开 Review。 */
  async function openSavedReview(review: SavedReviewSummary) {
    const pullRequest = review.pullRequest;
    const key = pullRequest
      ? pullRequestReviewKey(pullRequest.owner, pullRequest.repository, pullRequest.number)
      : `review:${review.reviewId}`;
    await withReviewOpening(key, async () => {
      setError("");
      try {
      const nextSnapshot = await invoke<ReviewSnapshot>("open_saved_review", {
        reviewId: review.reviewId,
      });
      replaceSnapshot(nextSnapshot);
      setSelectedPullRequest(nextSnapshot.pullRequest && {
        repository: {
          owner: nextSnapshot.pullRequest.owner,
          name: nextSnapshot.pullRequest.repository,
        },
        pullRequest: {
          number: nextSnapshot.pullRequest.number,
          title: nextSnapshot.pullRequest.title,
          description: nextSnapshot.pullRequest.description,
          author: nextSnapshot.pullRequest.author,
          state: nextSnapshot.pullRequest.state,
          draft: nextSnapshot.pullRequest.draft,
          url: nextSnapshot.pullRequest.url,
          updatedAt: nextSnapshot.pullRequest.updatedAt
            ?? new Date(nextSnapshot.pullRequest.synchronizedAt).toISOString(),
          base: { ref: nextSnapshot.pullRequest.baseRef, sha: nextSnapshot.baseRevision },
          head: { ref: nextSnapshot.pullRequest.headRef, sha: nextSnapshot.headRevision },
        },
      });
      setReviewUpdate(undefined);
      setFileChange(undefined);
      } catch (reason) {
        setError(String(reason));
      }
    });
  }

  /** 在 Repository Entry 已进入后打开 current Snapshot，并更新最近审核排序。 */
  async function reopenResolvedReview(summary: ReviewSummary) {
    replaceSnapshot(await invoke<ReviewSnapshot>("reopen_review", {
      reviewId: summary.reviewId,
    }));
    const reopened = { ...summary, lastOpenedAt: Date.now() };
    setRecentReviews((reviews) => [
      reopened,
      ...reviews.filter((review) => review.reviewId !== reopened.reviewId),
    ]);
    setRepositoryReviews((reviews) => [
      reopened,
      ...reviews.filter((review) => review.reviewId !== reopened.reviewId),
    ]);
    setRepositoryEntries((entries) => entries.map((entry) => entry.id === summary.repositoryEntryId
      ? { ...entry, lastOpenedAt: reopened.lastOpenedAt }
      : entry));
    setSelectedRepository((entry) => entry?.id === summary.repositoryEntryId
      ? { ...entry, lastOpenedAt: reopened.lastOpenedAt }
      : entry);
    void Promise.resolve(invoke<RepositoryEntry>("open_repository_entry", { id: summary.repositoryEntryId }))
      .catch(() => undefined);
    setFileChange(undefined);
  }

  /** 仅允许一个侧边栏仓库导航进行，成功后由 openRepository 提交主视图。 */
  async function navigateToRepository(repository: HomeRepository) {
    if (pendingNavigation || (selectedRepository?.id === repository.id && !snapshot)) return;
    const entry = repositoryEntries.find((candidate) => candidate.id === repository.id);
    if (!entry) return;
    setPendingNavigation({ type: "repository", id: repository.id });
    try {
      await openRepository(entry);
    } finally {
      setPendingNavigation(undefined);
    }
  }

  /** 仅允许一个侧边栏 Review 导航进行，失败时保留当前主视图。 */
  async function navigateToReview(review: ReviewSummary) {
    if (pendingNavigation || snapshot?.reviewId === review.reviewId) return;
    await withReviewOpening(`review:${review.reviewId}`, async () => {
      setPendingNavigation({ type: "review", id: review.reviewId });
      try {
        await reopenReview(review);
      } finally {
        setPendingNavigation(undefined);
      }
    });
  }

  /** 从持久化列表打开 current Snapshot，并在跨仓库进入时恢复 Repository Entry 上下文。 */
  async function reopenReview(summary: ReviewSummary) {
    setError("");
    try {
      if (selectedRepository?.id !== summary.repositoryEntryId) {
        const entry = repositoryEntries.find((candidate) => candidate.id === summary.repositoryEntryId);
        if (!entry) throw new Error("Review 所属 Repository Entry 不存在。");
        const opened = await invoke<RepositoryEntry>("open_repository_entry", { id: entry.id });
        const trusted = await invoke<boolean>("repository_workspace_trust", {
          repositoryEntryId: opened.id,
        });
        if (!trusted) {
          setPendingRepository(opened);
          setPendingReview(summary);
          return;
        }
        const nextSnapshot = await invoke<ReviewSnapshot>("reopen_review", {
          reviewId: summary.reviewId,
        });
        const [saved, reviews] = await Promise.all([
          invoke<SavedReviewSummary[]>("list_saved_reviews", { repositoryEntryId: opened.id }),
          invoke<ReviewSummary[]>("list_reviews", { repositoryEntryId: opened.id }),
        ]);
        setSelectedRepository(opened);
        setSavedReviews(saved ?? []);
        setRepositoryReviews(reviews ?? []);
        setRepositoryPath(opened.canonicalPath);
        setPullRequestGroups([]);
        setDiscoveryError(undefined);
        replaceSnapshot(nextSnapshot);
        const reopened = { ...summary, lastOpenedAt: Date.now() };
        setRecentReviews((candidates) => [
          reopened,
          ...candidates.filter((candidate) => candidate.reviewId !== reopened.reviewId),
        ]);
        setFileChange(undefined);
        void loadPullRequestGroups(opened);
        return;
      }
      await reopenResolvedReview(summary);
    } catch (reason) {
      setError(String(reason));
    }
  }

  /** 在风险 Dialog 明确确认后持久化 Workspace Trust，并进入待打开仓库。 */
  async function grantWorkspaceTrust() {
    if (!pendingRepository || grantingTrust) return;
    setError("");
    setGrantingTrust(true);
    try {
      await invoke("grant_repository_workspace_trust", {
        repositoryEntryId: pendingRepository.id,
      });
      const trustedRepository = pendingRepository;
      const reviewToResume = pendingReview;
      setPendingRepository(undefined);
      setPendingReview(undefined);
      await enterRepository(trustedRepository);
      if (reviewToResume) await reopenResolvedReview(reviewToResume);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setGrantingTrust(false);
    }
  }

  /** 将 discovery metadata 作为 Selection，并让代码内容完全走本地 Core Git。 */
  async function openDiscoveredPullRequest(
    repository: PullRequestGroup["repository"],
    pullRequest: DiscoveredPullRequest,
  ) {
    if (!selectedRepository) return;
    const key = pullRequestReviewKey(repository.owner, repository.name, pullRequest.number);
    await withReviewOpening(key, async () => {
      setError("");
      try {
      const nextSnapshot = await invoke<ReviewSnapshot>("open_pull_request_review", {
        repositoryEntryId: selectedRepository.id,
        repository,
        pullRequest,
      });
      setSelectedPullRequest({ repository, pullRequest });
      setReviewUpdate(undefined);
      replaceSnapshot(nextSnapshot);
      setFileChange(undefined);
      await loadSavedReviews(selectedRepository.id);
      } catch (reason) {
        setError(String(reason));
      }
    });
  }

  /** 重新查询 GitHub metadata 并检查候选 revisions，不改变当前 Snapshot。 */
  async function checkPullRequestUpdate() {
    if (!selectedRepository || !selectedPullRequest || !snapshot) return;
    const checkedReviewId = snapshot.reviewId;
    const checkedSnapshotId = snapshot.snapshotId;
    setReviewUpdateError("");
    try {
      const groups = await invoke<PullRequestGroup[]>("discover_github_pull_requests", {
        repositoryEntryId: selectedRepository.id,
      });
      if (activeReview.current?.reviewId !== checkedReviewId
        || activeSnapshotId.current !== checkedSnapshotId) return;
      setPullRequestGroups(groups);
      const identity = `${selectedPullRequest.repository.owner}/${selectedPullRequest.repository.name}`;
      const group = groups.find((candidate) => (
        `${candidate.repository.owner}/${candidate.repository.name}` === identity
      ));
      if (group?.error) throw new Error(`GitHub metadata：${group.error}`);
      const current = group?.pullRequests.find(
        (candidate) => candidate.number === selectedPullRequest.pullRequest.number,
      );
      if (!current) throw new Error("GitHub metadata：当前无法找到这个 Pull Request。");
      setSelectedPullRequest({ repository: group!.repository, pullRequest: current });
      const update = await invoke<ReviewUpdate>("check_pull_request_review_update", {
        reviewId: snapshot.reviewId,
        repository: group!.repository,
        pullRequest: current,
      });
      if (activeReview.current?.reviewId === checkedReviewId
        && activeSnapshotId.current === checkedSnapshotId
        && typeof update?.currentHeadRevision === "string") setReviewUpdate(update);
    } catch (reason) {
      if (activeReview.current?.reviewId === checkedReviewId
        && activeSnapshotId.current === checkedSnapshotId) {
        setReviewUpdateError(String(reason).replace(/^Error: /, ""));
      }
    }
  }

  /** 仅在用户明确确认后刷新 Snapshot、File Changes 与 Review Workspace。 */
  async function refreshPullRequestReview() {
    if (!selectedPullRequest || !snapshot || refreshingReview) return;
    setReviewUpdateError("");
    setRefreshingReview(true);
    try {
      replaceSnapshot(await invoke<ReviewSnapshot>("refresh_pull_request_review", {
        reviewId: snapshot.reviewId,
        repository: selectedPullRequest.repository,
        pullRequest: selectedPullRequest.pullRequest,
      }));
      setReviewUpdate(undefined);
      setFileChange(undefined);
    } catch (reason) {
      setReviewUpdateError(String(reason));
    } finally {
      setRefreshingReview(false);
    }
  }

  /** 启动连接向导，并在已配置时将用户带到 GitHub App 安装页。 */
  async function connectGitHub() {
    setError("");
    setCopiedValue(undefined);
    if (!testHarnessEnabled) setGitHubDialogOpen(true);
    try {
      const status = await invoke<GitHubAuthStatus>("github_auth_status");
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
      const authorization = await invoke<DeviceAuthorization>("start_github_authorization");
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
      await invoke("complete_github_authorization", {
        authorizationId: deviceAuthorization.authorizationId,
      });
      const status = await invoke<GitHubAuthStatus>("github_auth_status");
      setAuthStatus({ ...status, signedIn: true });
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
    try {
      await invoke("sign_out_github");
      setAuthStatus((status) => status ? { ...status, signedIn: false, identity: null } : status);
      setDeviceAuthorization(undefined);
      if (selectedRepository) await loadPullRequestGroups(selectedRepository);
    } catch (reason) {
      void invoke<GitHubAuthStatus>("github_auth_status")
        .then(setAuthStatus)
        .catch(() => undefined);
      setError(String(reason));
    }
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
    setBranchReloadError("");
    setBranchesReloading(true);
    try {
      const branches = await invokeLocalGit<BranchRefs>("list_local_refs", {
        repositoryEntryId,
      });
      if (!branches || !Array.isArray(branches.refs)) {
        throw new Error("分支应用边界返回了无效数据。");
      }
      setRefs(branches.refs);
      setDefaultRef(branches.defaultRef);
      setCurrentRef(branches.currentRef);
      replaceSnapshot(undefined);
      setFileChange(undefined);
      setBaseRef(branches.defaultRef);
      setHeadRef(branches.currentRef);
    } catch (reason) {
      setBranchReloadError(String(reason));
    } finally {
      setBranchesReloading(false);
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
    void invokeBackgroundGit<BranchReviewUpdate>("check_branch_review_updates", {
      reviewId: matchingBranchReview.reviewId,
    }).then((update) => {
      if (branchSelectionCheckId.current !== checkId) return;
      setBranchSelectionCheck(update.changed ? { status: "update", update } : { status: "current" });
    }).catch(() => {
      if (branchSelectionCheckId.current === checkId) setBranchSelectionCheck({ status: "failed" });
    }).finally(() => window.clearTimeout(messageTimer));
    return () => window.clearTimeout(messageTimer);
    // invokeBackgroundGit 的 identity 由每次调用生成；这里只由用户可见的 Selection 驱动。
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

  /** 统一呈现新 Snapshot，并在提供摘要时同步仓库与最近审核列表。 */
  function presentReviewSnapshot(nextSnapshot: ReviewSnapshot, summary?: ReviewSummary) {
    replaceSnapshot(nextSnapshot);
    setFileChange(undefined);
    if (!summary) return;
    setRepositoryReviews((reviews) => [
      summary,
      ...reviews.filter((review) => review.reviewId !== summary.reviewId),
    ]);
    setRecentReviews((reviews) => [
      summary,
      ...reviews.filter((review) => review.reviewId !== summary.reviewId),
    ]);
  }

  async function openComparison() {
    const key = `branch:${baseRef}:${headRef}`;
    await withReviewOpening(key, async () => {
      setError("");
      try {
      const nextSnapshot = await invokeLocalGit<ReviewSnapshot>("compare_local_refs", {
        repositoryEntryId: selectedRepository?.id,
        baseRef,
        headRef,
      });
      if (selectedRepository) {
        const summary: ReviewSummary = {
          reviewId: nextSnapshot.reviewId,
          repositoryEntryId: selectedRepository.id,
          repositoryPath: selectedRepository.canonicalPath,
          baseRef: nextSnapshot.baseRef,
          headRef: nextSnapshot.headRef,
          lastOpenedAt: Date.now(),
        };
        presentReviewSnapshot(nextSnapshot, summary);
      } else {
        presentReviewSnapshot(nextSnapshot);
      }
      if (selectedRepository) await loadSavedReviews(selectedRepository.id);
    }
      catch (reason) { replaceSnapshot(undefined); setFileChange(undefined); setError(String(reason)); }
    });
  }

  /** 根据已确认的 moving refs 创建新 Snapshot，再进入同一个 durable Review。 */
  async function updateAndOpenBranchReview() {
    if (!matchingBranchReview || branchSelectionCheck.status !== "update") return;
    const key = `branch:${baseRef}:${headRef}`;
    await withReviewOpening(key, async () => {
      setError("");
      try {
      await invoke<ReviewSnapshot>("reopen_review", { reviewId: matchingBranchReview.reviewId });
      const nextSnapshot = await invokeLocalGit<ReviewSnapshot>("refresh_branch_review", {
        reviewId: matchingBranchReview.reviewId,
        confirmation: branchSelectionCheck.update,
      });
      if (selectedRepository) {
        const summary: ReviewSummary = {
          ...matchingBranchReview,
          lastOpenedAt: Date.now(),
          baseRef: nextSnapshot.baseRef,
          headRef: nextSnapshot.headRef,
        };
        presentReviewSnapshot(nextSnapshot, summary);
        await loadSavedReviews(selectedRepository.id);
      } else {
        presentReviewSnapshot(nextSnapshot);
      }
      } catch (reason) {
      void Promise.resolve(invoke("close_review", { reviewId: matchingBranchReview.reviewId })).catch(() => undefined);
      replaceSnapshot(undefined);
      setFileChange(undefined);
      setError(String(reason));
      }
    });
  }

  async function openFileChange(path: string) {
    if (!snapshot) return;
    const requestId = ++fileChangeRequestId.current;
    setFileChangeError("");
    setFileChangeLoading(true);
    setPendingFilePath(path);
    try {
      const loaded = await invokeLocalGit<FileChangeContent>("read_review_file_change", {
        reviewId: snapshot.reviewId,
        snapshotId: snapshot.snapshotId,
        path,
      });
      if (fileChangeRequestId.current === requestId) setFileChange(loaded);
    } catch (reason) {
      if (fileChangeRequestId.current === requestId) {
        setFileChange(undefined);
        setFileChangeError(String(reason));
      }
    } finally {
      if (fileChangeRequestId.current === requestId) {
        setFileChangeLoading(false);
        setPendingFilePath(undefined);
      }
    }
  }
  openFileChangeRef.current = openFileChange;

  /** 打开或刷新 Review 后按稳定路径顺序选择第一个 File Change。 */
  const firstFileChangePath = snapshot
    ? [...snapshot.fileChanges].sort((left, right) => comparePaths(left.path, right.path))[0]?.path
    : undefined;
  useEffect(() => {
    if (!snapshot?.reviewId) return;
    if (!firstFileChangePath) {
      setFileChange(undefined);
      return;
    }
    void openFileChangeRef.current?.(firstFileChangePath);
  }, [snapshot?.reviewId, snapshot?.snapshotId, firstFileChangePath]);

  /** 只检查 Branch Selection 的 moving refs，不改变正在展示的 Snapshot。 */
  async function checkBranchReviewUpdates() {
    if (!snapshot) return;
    const checkedReviewId = snapshot.reviewId;
    const checkedSnapshotId = snapshot.snapshotId;
    setReviewUpdateError("");
    try {
      const update = await invokeBackgroundGit<BranchReviewUpdate>("check_branch_review_updates", {
        reviewId: snapshot.reviewId,
      });
      if (activeReview.current?.reviewId === checkedReviewId
        && activeSnapshotId.current === checkedSnapshotId
        && typeof update?.currentHeadRevision === "string") setBranchUpdate(update);
    } catch (reason) {
      if (activeReview.current?.reviewId === checkedReviewId
        && activeSnapshotId.current === checkedSnapshotId) {
        setBranchUpdate(undefined);
        setReviewUpdateError(String(reason));
      }
    }
  }

  /** 在用户明确确认后刷新 Snapshot、File Changes 与 Review Workspace。 */
  async function refreshBranchReview() {
    if (!snapshot || refreshingReview) return;
    setReviewUpdateError("");
    setRefreshingReview(true);
    try {
      const nextSnapshot = await invokeLocalGit<ReviewSnapshot>("refresh_branch_review", {
        reviewId: snapshot.reviewId,
        confirmation: branchUpdate,
      });
      presentReviewSnapshot(nextSnapshot);
    } catch (reason) {
      setReviewUpdateError(String(reason));
    } finally {
      setRefreshingReview(false);
    }
  }

  /** 打开已有 Review 后非阻塞检查 moving Selection；检查期间继续展示 current Snapshot。 */
  useEffect(() => {
    if (!snapshot) return;
    if (snapshot.pullRequest && selectedPullRequest) {
      void checkPullRequestUpdate();
    } else if (!snapshot.pullRequest) {
      void checkBranchReviewUpdates();
    }
    // 检查只由 immutable Snapshot identity 驱动，避免 discovery state 更新造成重复请求。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snapshot ? `${snapshot.reviewId}:${snapshot.snapshotId ?? `${snapshot.baseRevision}:${snapshot.headRevision}`}` : undefined]);

  /** 统一将完整 object id 压缩为用户可辨认的短 revision。 */
  function shortRevision(revision: string) {
    return revision.slice(0, 8);
  }

  return <AppShell
    enabled={catalogRuntimeEnabled && (!testHarnessEnabled || Boolean(selectedRepository))}
    fallbackClassName="narrant-app"
    showHome={!selectedRepository && !testHarnessEnabled}
    activeRepositoryId={selectedRepository?.id}
    activeReviewId={snapshot?.reviewId}
    pendingRepositoryId={pendingNavigation?.type === "repository" ? pendingNavigation.id : undefined}
    pendingReviewId={reviewOpeningKey?.startsWith("review:") ? reviewOpeningKey.slice("review:".length) : undefined}
    reviewNavigationBusy={Boolean(reviewOpeningKey) || refreshingReview || grantingTrust}
    workspace={Boolean(snapshot)}
    reviews={recentReviews}
    repositories={repositoryEntries}
    error={!selectedRepository ? error : undefined}
    loading={catalogLoading}
    onOpenHome={leaveRepository}
    onOpenReview={(review) => void navigateToReview(review)}
    onOpenRepository={(repository) => void navigateToRepository(repository)}
    onOpenFolder={(path) => void registerRepositoryFolder(path)}
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
            <Button variant="outline" disabled={!entry.available} onClick={() => openRepository(entry)}>
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
              <span className="mr-3">{review.repositoryPath}</span><BranchRelation baseRef={review.baseRef} headRef={review.headRef} />
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
        initialTab={repositoryInitialTab}
        openingReviewKey={reviewOpeningKey}
        reviewNavigationBusy={Boolean(reviewOpeningKey) || refreshingReview || grantingTrust}
      />
    </article>}
    <Dialog open={Boolean(pendingRepository)} onOpenChange={(open) => {
      if (!open && !grantingTrust) {
        setPendingRepository(undefined);
        setPendingReview(undefined);
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
            onClick={() => {
              setPendingRepository(undefined);
              setPendingReview(undefined);
            }}
          >
            {t("cancel")}
          </Button>
          <Button aria-busy={grantingTrust || undefined} disabled={grantingTrust} onClick={() => void grantWorkspaceTrust()}>
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
    {error && <ErrorMessage message={error} />}
    {snapshot && <div className="h-[calc(100vh-4.5rem)]">
      <ReviewReader
        key={`${snapshot.reviewId}:${snapshot.snapshotId}`}
        snapshot={snapshot}
        fileChange={fileChange}
        pendingFilePath={pendingFilePath}
        fileChangeLoading={fileChangeLoading}
        fileChangeError={fileChangeError}
        firstFileChangePath={firstFileChangePath}
        navigationBusy={Boolean(reviewOpeningKey) || refreshingReview || grantingTrust}
        diffMode={diffMode}
        narrow={narrowReview}
        editorTheme={editorTheme}
        initialTreeWidth={initialTreeWidth}
        updateAction={<>
          {snapshot.pullRequest && reviewUpdate?.updateAvailable && <div role="status" className="flex items-center gap-2 text-xs">
            <span className="text-amber-700 dark:text-amber-400">{t("review:currentAvailable", { current: shortRevision(reviewUpdate.currentHeadRevision), available: shortRevision(reviewUpdate.availableHeadRevision) })}</span>
            <Button size="xs" disabled={refreshingReview || Boolean(reviewOpeningKey)} aria-busy={refreshingReview || undefined} onClick={() => void refreshPullRequestReview()}>{refreshingReview && <Spinner aria-hidden />}{refreshingReview ? t("review:refreshing") : t("review:refresh")}</Button>
          </div>}
          {!snapshot.pullRequest && branchUpdate?.changed && <div role="status" className="flex items-center gap-2 text-xs">
            <span className="text-amber-700 dark:text-amber-400">{t("review:currentAvailable", { current: shortRevision(branchUpdate.currentHeadRevision), available: shortRevision(branchUpdate.latestHeadRevision) })}</span>
            <Button size="xs" disabled={refreshingReview || Boolean(reviewOpeningKey)} aria-busy={refreshingReview || undefined} onClick={refreshBranchReview}>{refreshingReview && <Spinner aria-hidden />}{refreshingReview ? t("review:refreshing") : t("review:refresh")}</Button>
          </div>}
        </>}
        notice={reviewUpdateError ? <p role="alert" className="text-destructive">{reviewUpdateError}</p> : undefined}
        onSelectFile={(path) => void openFileChange(path)}
        onRetryFile={(path) => void openFileChange(path)}
        onDiffModeChange={selectDiffMode}
        onTreeWidthChange={(width) => localStorage.setItem(REVIEW_TREE_WIDTH_KEY, String(width))}
        onOpenExternal={(url) => void openUrl(url)}
        onClose={() => closeReviewView()}
      />
    </div>}
    </>}
  </AppShell>;
}

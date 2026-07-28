import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DiffEditor } from "@monaco-editor/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useTranslation } from "react-i18next";
import { Button, buttonVariants } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { useSystemTheme } from "@/lib/use-system-theme";
import { HomePage } from "@/components/home-page";
import { RepositoryOverview } from "@/components/repository-overview";
import type { DiscoveredPullRequest, PullRequestGroup, PullRequestMetadata, ReviewSummary, SavedReviewSummary } from "@/lib/review-types";
import "@/lib/i18n";

type ChangeStatus = "added" | "modified" | "deleted" | "renamed";
type FileChangeSummary = {
  path: string;
  oldPath: string | null;
  status: ChangeStatus;
};
type FileChangeContent = FileChangeSummary & {
  contentKind: "text" | "binary" | "submodule" | "symlink";
  beforeSize: number | null;
  afterSize: number | null;
  before: string | null;
  after: string | null;
};
type ReviewSnapshotBase = {
  baseRef: string;
  baseRevision: string;
  comparisonBase: string;
  headRef: string;
  headRevision: string;
  fileChanges: FileChangeSummary[];
};
type ReviewSnapshot = ReviewSnapshotBase & {
  reviewId: string;
  snapshotId: string;
  pullRequest?: PullRequestMetadata;
};
type BranchReviewUpdate = {
  changed: boolean;
  currentBaseRevision: string;
  currentHeadRevision: string;
  latestBaseRevision: string;
  latestHeadRevision: string;
};
/** 应用边界显式返回的分支选项与 Repository Entry 默认选择。 */
type BranchRefs = {
  refs: string[];
  defaultRef: string;
  currentRef: string;
};
type ActiveReview = { reviewId: string };
type GitHubAuthStatus = {
  configured: boolean;
  signedIn: boolean;
  appInstallUrl: string | null;
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

type DirectoryNode = {
  name: string;
  directories: DirectoryNode[];
  files: FileChangeSummary[];
};

/** Monaco Diff 阅读器支持的持久化展示布局。 */
type DiffMode = "side-by-side" | "inline";

const REVIEW_TREE_WIDTH_KEY = "narrant.review.fileTreeWidth";
const REVIEW_DIFF_MODE_KEY = "narrant.review.diffMode";

/** 以不依赖系统 locale 的代码点顺序稳定排列 File Change 路径。 */
function comparePaths(left: string, right: string) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function buildDirectoryTree(fileChanges: FileChangeSummary[]): DirectoryNode {
  const root: DirectoryNode = { name: "", directories: [], files: [] };
  for (const change of fileChanges) {
    const parts = change.path.split("/");
    let directory = root;
    for (const name of parts.slice(0, -1)) {
      let child = directory.directories.find((candidate) => candidate.name === name);
      if (!child) {
        child = { name, directories: [], files: [] };
        directory.directories.push(child);
      }
      directory = child;
    }
    directory.files.push(change);
  }
  const sort = (directory: DirectoryNode) => {
    directory.directories.sort((left, right) => comparePaths(left.name, right.name));
    directory.files.sort((left, right) => comparePaths(left.path, right.path));
    directory.directories.forEach(sort);
  };
  sort(root);
  return root;
}

function ErrorMessage({ message }: { message: string }) {
  const match = message.match(/(https:\/\/github\.com\/[^\s]+)/);
  const text = match ? message.replace(match[1], "") : message;
  return <p role="alert">
    {text}
    {match && <> <a href={match[1]} onClick={(event) => { event.preventDefault(); void openUrl(match[1]); }}>在 GitHub 查看</a></>}
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
  const [pendingRepository, setPendingRepository] = useState<RepositoryEntry>();
  const [pendingReview, setPendingReview] = useState<ReviewSummary>();
  const [workspaceTrusted, setWorkspaceTrusted] = useState(false);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [pullRequestGroups, setPullRequestGroups] = useState<PullRequestGroup[]>([]);
  const [savedReviews, setSavedReviews] = useState<SavedReviewSummary[]>([]);
  const [discoveryLoading, setDiscoveryLoading] = useState(false);
  const [discoveryError, setDiscoveryError] = useState<string>();
  const [refs, setRefs] = useState<string[]>([]);
  const [baseRef, setBaseRef] = useState("");
  const [headRef, setHeadRef] = useState("");
  const [snapshot, setSnapshot] = useState<ReviewSnapshot>();
  const [selectedPullRequest, setSelectedPullRequest] = useState<SelectedPullRequest>();
  const [reviewUpdate, setReviewUpdate] = useState<ReviewUpdate>();
  const [branchUpdate, setBranchUpdate] = useState<BranchReviewUpdate>();
  const [reviewUpdateError, setReviewUpdateError] = useState("");
  const [refreshingReview, setRefreshingReview] = useState(false);
  const [repositoryInitialTab, setRepositoryInitialTab] = useState<"pull-requests" | "branches" | "history">("pull-requests");
  const [fileChange, setFileChange] = useState<FileChangeContent>();
  const [error, setError] = useState("");
  const [authStatus, setAuthStatus] = useState<GitHubAuthStatus>();
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
  function closeReviewView() {
    replaceSnapshot(undefined);
    setFileChange(undefined);
    setReviewUpdate(undefined);
    setReviewUpdateError("");
    setRepositoryInitialTab("history");
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

  /** 验证并持久化用户输入的具体本地 clone。 */
  async function registerRepository() {
    setError("");
    setCatalogLoading(true);
    try {
      const entry = await invoke<RepositoryEntry>("register_repository_entry", {
        path: repositoryPath,
      });
      setRepositoryEntries((entries) => [
        entry,
        ...entries.filter((candidate) => candidate.id !== entry.id),
      ]);
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
      setRepositoryEntries((entries) => [entry, ...entries.filter((candidate) => candidate.id !== entry.id)]);
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
    setWorkspaceTrusted(true);
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
    try {
      const opened = await invoke<RepositoryEntry>("open_repository_entry", { id: entry.id });
      setRepositoryEntries((entries) => entries.map((candidate) => (
        candidate.id === opened.id ? opened : candidate
      )));
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
      return "entered";
    } catch (reason) {
      setError(String(reason));
      return "failed";
    }
  }

  /** 不依赖 GitHub 登录或 discovery，从 cached current Snapshot 重开 Review。 */
  async function openSavedReview(review: SavedReviewSummary) {
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
          base: { ref: nextSnapshot.pullRequest.baseRef, sha: nextSnapshot.baseRevision },
          head: { ref: nextSnapshot.pullRequest.headRef, sha: nextSnapshot.headRevision },
        },
      });
      setReviewUpdate(undefined);
      setFileChange(undefined);
    } catch (reason) {
      setError(String(reason));
    }
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
    setFileChange(undefined);
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
        setWorkspaceTrusted(true);
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
    if (!pendingRepository) return;
    setError("");
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
    }
  }

  /** 将 discovery metadata 作为 Selection，并让代码内容完全走本地 Core Git。 */
  async function openDiscoveredPullRequest(
    repository: PullRequestGroup["repository"],
    pullRequest: DiscoveredPullRequest,
  ) {
    if (!selectedRepository) return;
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
    setError("");
    try {
      await navigator.clipboard.writeText(value);
      setCopiedValue(copied);
    } catch (reason) {
      setError(`无法复制到剪贴板：${String(reason)}`);
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
      setAuthStatus((status) => status ? { ...status, signedIn: true } : status);
      setDeviceAuthorization(undefined);
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
      setAuthStatus((status) => status ? { ...status, signedIn: false } : status);
      setDeviceAuthorization(undefined);
      if (selectedRepository) await loadPullRequestGroups(selectedRepository);
    } catch (reason) {
      setError(String(reason));
    }
  }

  /** 渲染由用户主动打开的 GitHub 安装、Device Flow、复制与退出入口。 */
  function renderGitHubControls() {
    return <div className="space-y-3 text-sm">
      {!authStatus && <Button variant="ghost" className="w-full justify-start" onClick={connectGitHub}>连接 GitHub</Button>}
      {authStatus?.signedIn && <Button variant="ghost" className="w-full justify-start" onClick={signOutGitHub}>退出 GitHub</Button>}
      {authStatus && !authStatus.signedIn && (!authStatus.configured
        ? <p className="px-2 text-xs text-muted-foreground">此构建未配置 GitHub App；公开 Pull Request 仍可匿名发现。</p>
        : <>
          <p className="px-2 text-xs">先安装 Narrant GitHub App 并为目标仓库授予只读访问；组织仓库可能需要管理员审批，再授权当前账号。</p>
          {authStatus.appInstallUrl && <Button variant="outline" className="w-full" onClick={() => openUrl(authStatus.appInstallUrl!)}>重新选择仓库或申请组织审批</Button>}
          <Button className="w-full" disabled={authorizing} onClick={startGitHubAuthorization}>我已安装，继续</Button>
          {deviceAuthorization && <div className="space-y-2 rounded-md border p-2">
            <p>设备码：<strong>{deviceAuthorization.userCode}</strong></p>
            <Button variant="outline" className="w-full" onClick={() => void copyAuthorizationValue(deviceAuthorization.userCode, "device-code")}>
              {copiedValue === "device-code" ? "设备码已复制" : "复制设备码"}
            </Button>
            <Button variant="outline" className="w-full" onClick={() => void copyAuthorizationValue(deviceAuthorization.verificationUri, "authorization-url")}>
              {copiedValue === "authorization-url" ? "链接已复制" : "复制链接"}
            </Button>
            <a href={deviceAuthorization.verificationUri} onClick={(event) => { event.preventDefault(); void openUrl(deviceAuthorization.verificationUri); }}>{deviceAuthorization.verificationUri}</a>
            <Button variant="outline" className="w-full" onClick={() => void openUrl(deviceAuthorization.verificationUri)}>打开 GitHub</Button>
            <p className="text-xs text-muted-foreground">请核对应用名称，不要输入他人发送的验证码。</p>
            <Button className="w-full" disabled={authorizing} onClick={completeGitHubAuthorization}>我已在 GitHub 完成授权</Button>
          </div>}
        </>)}
    </div>;
  }

  /** 加载 canonical refs，并使用后端识别的默认分支和当前检出分支初始化选择。 */
  async function loadRefs(repositoryEntryId = selectedRepository?.id) {
    setError("");
    try {
      const branches = await invokeLocalGit<BranchRefs>("list_local_refs", {
        repositoryEntryId,
      });
      if (!branches || !Array.isArray(branches.refs)) {
        throw new Error("分支应用边界返回了无效数据。");
      }
      setRefs(branches.refs);
      replaceSnapshot(undefined);
      setFileChange(undefined);
      setBaseRef(branches.defaultRef);
      setHeadRef(branches.currentRef);
    } catch (reason) {
      setRefs([]);
      replaceSnapshot(undefined);
      setFileChange(undefined);
      setError(String(reason));
    }
  }

  const hasMatchingBranchReview = repositoryReviews.some(
    (review) => review.baseRef === baseRef && review.headRef === headRef,
  );
  const branchSelectionInvalid = Boolean(baseRef && headRef && baseRef === headRef);

  async function openComparison() {
    setError("");
    try {
      const nextSnapshot = await invokeLocalGit<ReviewSnapshot>("compare_local_refs", {
        repositoryEntryId: selectedRepository?.id,
        baseRef,
        headRef,
      });
      replaceSnapshot(nextSnapshot);
      if (selectedRepository) {
        const summary: ReviewSummary = {
          reviewId: nextSnapshot.reviewId,
          repositoryEntryId: selectedRepository.id,
          repositoryPath: selectedRepository.canonicalPath,
          baseRef: nextSnapshot.baseRef,
          headRef: nextSnapshot.headRef,
          lastOpenedAt: Date.now(),
        };
        setRepositoryReviews((reviews) => [
          summary,
          ...reviews.filter((review) => review.reviewId !== summary.reviewId),
        ]);
        setRecentReviews((reviews) => [
          summary,
          ...reviews.filter((review) => review.reviewId !== summary.reviewId),
        ]);
      }
      setFileChange(undefined);
      if (selectedRepository) await loadSavedReviews(selectedRepository.id);
    }
    catch (reason) { replaceSnapshot(undefined); setFileChange(undefined); setError(String(reason)); }
  }

  async function openFileChange(path: string) {
    if (!snapshot) return;
    setError("");
    try {
      setFileChange(await invokeLocalGit<FileChangeContent>("read_review_file_change", {
        reviewId: snapshot.reviewId,
        snapshotId: snapshot.snapshotId,
        path,
      }));
    } catch (reason) {
      setFileChange(undefined);
      setError(String(reason));
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
      replaceSnapshot(await invokeLocalGit<ReviewSnapshot>("refresh_branch_review", {
        reviewId: snapshot.reviewId,
        confirmation: branchUpdate,
      }));
      setFileChange(undefined);
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

  /** 使用颜色与本地化文字共同表达 File Change 状态。 */
  function renderChangeStatus(status: ChangeStatus) {
    const colors: Record<ChangeStatus, string> = {
      added: "border-green-500/30 bg-green-500/10 text-green-700 dark:text-green-400",
      deleted: "border-red-500/30 bg-red-500/10 text-red-700 dark:text-red-400",
      modified: "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-400",
      renamed: "border-blue-500/30 bg-blue-500/10 text-blue-700 dark:text-blue-400",
    };
    return <Badge variant="outline" className={colors[status]}>{t(`review:${status}`)}</Badge>;
  }

  function renderDirectory(directory: DirectoryNode) {
    return <>
      {directory.directories.map((child) => <li key={child.name} role="treeitem" aria-label={child.name} aria-expanded="true">
        <span>{child.name}</span>
        <ul role="group">{renderDirectory(child)}</ul>
      </li>)}
      {directory.files.map((change) => <li key={change.path} role="none">
        <Button
          variant="ghost"
          type="button"
          role="treeitem"
          aria-current={fileChange?.path === change.path ? "true" : undefined}
          onClick={() => openFileChange(change.path)}
        >
          <span>{change.oldPath ? `${change.oldPath} → ${change.path}` : change.path.split("/").at(-1)}</span>
          {renderChangeStatus(change.status)}
        </Button>
      </li>)}
    </>;
  }

  return <main>
    <header><h1>Narrant</h1><p>只读变更审核</p></header>
    {catalogRuntimeEnabled && !selectedRepository && !testHarnessEnabled && <HomePage reviews={recentReviews} error={error} onOpenReview={(review) => void reopenReview(review)} onOpenFolder={(path) => void registerRepositoryFolder(path)} />}
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
            <Button variant="outline" onClick={() => void reopenReview(review)}>
              {review.repositoryPath} · {review.baseRef.split("/").at(-1)} → {review.headRef.split("/").at(-1)}
            </Button>
          </li>)}</ul>}
      </section>
    </article>}
    {selectedRepository && !snapshot && <article aria-label="Repository Entry">
      <Button variant="ghost" className="mb-3" onClick={leaveRepository}>返回 Repository Entries</Button>
      <RepositoryOverview
        repositoryName={selectedRepository.canonicalPath.split(/[\\/]/).filter(Boolean).at(-1) ?? selectedRepository.canonicalPath}
        repositoryPath={selectedRepository.canonicalPath}
        savedReviews={savedReviews}
        pullRequestGroups={pullRequestGroups}
        repositoryReviews={repositoryReviews}
        discoveryLoading={discoveryLoading}
        discoveryError={discoveryError}
        githubConnected={authStatus?.signedIn ?? false}
        githubControls={renderGitHubControls()}
        onRetryDiscovery={() => void loadPullRequestGroups(selectedRepository)}
        onConnectGitHub={() => void connectGitHub()}
        onOpenSavedReview={(review) => void openSavedReview(review)}
        onOpenPullRequest={(repository, pullRequest) => void openDiscoveredPullRequest(repository, pullRequest)}
        localBranches={<section aria-label="选择本地仓库" className="flex flex-wrap items-end gap-3">
          <label>本地仓库路径<Input aria-label="本地仓库路径" value={repositoryPath} readOnly /></label>
          <Button variant="outline" onClick={() => void loadRefs()}>重新读取分支</Button>
          <label>Base Ref<Select value={baseRef} onValueChange={(value) => setBaseRef(value ?? "")}><SelectTrigger aria-label="Base Ref"><SelectValue placeholder="选择 Base Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
          <label>Head Ref<Select value={headRef} onValueChange={(value) => setHeadRef(value ?? "")}><SelectTrigger aria-label="Head Ref"><SelectValue placeholder="选择 Head Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
          <Button disabled={!baseRef || !headRef || branchSelectionInvalid || !workspaceTrusted} onClick={openComparison}>
            {hasMatchingBranchReview ? t("continueReview") : t("startReview")}
          </Button>
          {branchSelectionInvalid && <p role="status">{t("sameBranchSelection")}</p>}
        </section>}
        initialTab={repositoryInitialTab}
      />
    </article>}
    <Dialog open={Boolean(pendingRepository)} onOpenChange={(open) => {
      if (!open) {
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
          <DialogClose className={buttonVariants({ variant: "outline" })}>{t("cancel")}</DialogClose>
          <Button onClick={() => void grantWorkspaceTrust()}>{t("trustAndOpen")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
    {!catalogRuntimeEnabled && !testHarnessEnabled && <p role="alert">Narrant 需要在桌面应用中运行。</p>}
    {(testHarnessEnabled || selectedRepository) && <>
    {testHarnessEnabled && <section aria-label="打开 GitHub Pull Request">{renderGitHubControls()}</section>}
    {snapshot && repositoryReviews.length > 1 && <nav aria-label="切换审核">
      {repositoryReviews
        .filter((review) => review.reviewId !== snapshot.reviewId)
        .map((review) => <Button
          key={review.reviewId}
          variant="ghost"
          onClick={() => void reopenReview(review)}
        >
          {review.baseRef.split("/").at(-1)} → {review.headRef.split("/").at(-1)}
        </Button>)}
    </nav>}
    {!selectedRepository && <><p>本地分支比较</p>
      <section aria-label="选择本地仓库" className="flex flex-wrap items-end gap-3">
        <label>本地仓库路径<Input aria-label="本地仓库路径" value={repositoryPath} onChange={(event) => setRepositoryPath(event.target.value)} /></label>
        <Button variant="outline" onClick={() => void loadRefs()}>重新读取分支</Button>
        <label>Base Ref<Select value={baseRef} onValueChange={(value) => setBaseRef(value ?? "")}><SelectTrigger aria-label="Base Ref"><SelectValue placeholder="选择 Base Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
        <label>Head Ref<Select value={headRef} onValueChange={(value) => setHeadRef(value ?? "")}><SelectTrigger aria-label="Head Ref"><SelectValue placeholder="选择 Head Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
        <Button disabled={!baseRef || !headRef || branchSelectionInvalid} onClick={openComparison}>
          {hasMatchingBranchReview ? t("continueReview") : t("startReview")}
        </Button>
        {branchSelectionInvalid && <p role="status">{t("sameBranchSelection")}</p>}
      </section>
    </>}
    {error && <ErrorMessage message={error} />}
    {snapshot && <details className="w-fit">
      <summary className={buttonVariants({ variant: "outline" })}>更多操作</summary>
      <Button variant="ghost" onClick={closeReviewView}>关闭审核</Button>
    </details>}
    {snapshot?.pullRequest && <article aria-label="Pull Request 概览">
      <h2>{snapshot.pullRequest.title}</h2>
      <p>#{snapshot.pullRequest.number} · {snapshot.pullRequest.author} · {snapshot.pullRequest.draft ? "草稿 · " : ""}{{ open: "开放", closed: "已关闭", merged: "已合并" }[snapshot.pullRequest.state ?? "open"]}</p>
      <p>{snapshot.baseRef} @ {snapshot.baseRevision} → {snapshot.headRef} @ {snapshot.headRevision}</p>
      <div>
        {reviewUpdate && (reviewUpdate.updateAvailable
          ? <div role="status">
            <p>当前 {shortRevision(reviewUpdate.currentHeadRevision)} · 可用 {shortRevision(reviewUpdate.availableHeadRevision)}</p>
            <Button disabled={refreshingReview} onClick={() => void refreshPullRequestReview()}>
              {refreshingReview ? "正在刷新审核…" : "刷新审核"}
            </Button>
          </div>
          : <p role="status">当前 {shortRevision(reviewUpdate.currentHeadRevision)} · 已是最新版本</p>)}
      </div>
      <div aria-label="Pull Request 描述">
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          skipHtml
          components={{
            img: ({ alt }) => <span>{alt ? `[图片：${alt}，已阻止远程加载]` : "[已阻止远程图片]"}</span>,
            a: ({ href, children }) => <a href={href} onClick={(event) => {
              event.preventDefault();
              if (href?.startsWith("https://") || href?.startsWith("http://")) void openUrl(href);
            }}>{children}</a>,
          }}
        >{snapshot.pullRequest.description}</ReactMarkdown>
      </div>
    </article>}
    {snapshot && !snapshot.pullRequest && <section aria-label="Branch Review 更新">
      <p>{snapshot.baseRef} → {snapshot.headRef}</p>
      {branchUpdate && !branchUpdate.changed && <p role="status">当前已是最新版本</p>}
      {branchUpdate?.changed && <div role="status">
        <p>当前 {shortRevision(branchUpdate.currentHeadRevision)} · 可用 {shortRevision(branchUpdate.latestHeadRevision)}</p>
        <Button disabled={refreshingReview} onClick={refreshBranchReview}>
          {refreshingReview ? "正在刷新审核…" : "刷新审核"}
        </Button>
      </div>}
    </section>}
    {snapshot && reviewUpdateError && <p role="alert" className="text-sm text-destructive">{reviewUpdateError}</p>}
    {snapshot && <section aria-label={t("review:reader")} className="mt-3 overflow-hidden rounded-lg border">
      <header className="flex min-h-11 items-center justify-between gap-3 border-b px-3 py-2">
        <div className="min-w-0">
          <h2 className="truncate text-sm font-semibold">
            {snapshot.pullRequest ? `#${snapshot.pullRequest.number} ${snapshot.pullRequest.title}` : `${snapshot.baseRef} → ${snapshot.headRef}`}
          </h2>
          <p className="truncate font-mono text-xs text-muted-foreground">
            {(snapshot.baseRevision ?? snapshot.comparisonBase).slice(0, 8)} → {snapshot.headRevision.slice(0, 8)}
          </p>
        </div>
        <div aria-label={t("review:diffLayout")} className="flex gap-1">
          <Button size="xs" variant={diffMode === "inline" ? "secondary" : "ghost"} aria-pressed={diffMode === "inline"} onClick={() => selectDiffMode("inline")}>{t("review:inline")}</Button>
          <Button size="xs" variant={diffMode === "side-by-side" ? "secondary" : "ghost"} aria-pressed={diffMode === "side-by-side"} onClick={() => selectDiffMode("side-by-side")}>{t("review:sideBySide")}</Button>
        </div>
      </header>
      {snapshot.fileChanges.length === 0
        ? <div className="grid min-h-72 place-content-center px-6 text-center">
          <h3 className="font-medium">{t("review:emptyTitle")}</h3>
          <p className="mt-1 text-sm text-muted-foreground">{t("review:emptyDescription")}</p>
        </div>
        : <ResizablePanelGroup
          orientation="horizontal"
          className="min-h-[32rem]"
          defaultLayout={{ "review-file-tree": initialTreeWidth, "review-diff": 100 - initialTreeWidth }}
          onLayoutChanged={(layout, meta) => {
            if (meta.isUserInteraction) {
              localStorage.setItem(REVIEW_TREE_WIDTH_KEY, String(layout["review-file-tree"]));
            }
          }}
        >
          <ResizablePanel id="review-file-tree" minSize="15%" maxSize="45%" defaultSize={`${initialTreeWidth}%`}>
            <nav aria-label={t("review:fileTree")} className="h-full overflow-auto border-r bg-muted/20">
              <div className="sticky top-0 border-b bg-background/95 px-3 py-2 backdrop-blur">
                <h3 className="text-xs font-semibold">{t("review:fileChanges")}</h3>
                <p className="text-xs text-muted-foreground">{t("review:changeCount", { count: snapshot.fileChanges.length })}</p>
              </div>
              <ul role="tree" aria-label={t("review:fileTree")} className="p-1">
                {renderDirectory(buildDirectoryTree(snapshot.fileChanges))}
              </ul>
            </nav>
          </ResizablePanel>
          <ResizableHandle withHandle aria-label={t("review:resizeFileTree")} />
          <ResizablePanel id="review-diff" minSize="45%">
            {fileChange && <section className="h-full min-w-0" aria-label="File Change" data-before={import.meta.env.VITE_WDIO ? fileChange.before ?? "" : undefined} data-after={import.meta.env.VITE_WDIO ? fileChange.after ?? "" : undefined}>
              <header className="border-b px-3 py-2">
                <h3 className="flex items-center gap-2 truncate font-mono text-sm font-medium">{fileChange.path} {renderChangeStatus(fileChange.status)}</h3>
              </header>
              {fileChange.contentKind === "binary" && <div className="grid min-h-72 place-content-center px-6 text-center"><p>{t("review:binary", { before: fileChange.beforeSize === null ? t("review:none") : t("review:bytes", { count: fileChange.beforeSize }), after: fileChange.afterSize === null ? t("review:none") : t("review:bytes", { count: fileChange.afterSize }) })}</p></div>}
              {fileChange.contentKind === "submodule" && <div className="grid min-h-72 place-content-center px-6 text-center"><h4 className="font-medium">{t("review:submodule")}</h4><p className="font-mono text-sm text-muted-foreground">{fileChange.before ?? t("review:none")} → {fileChange.after ?? t("review:none")}</p></div>}
              {fileChange.contentKind === "symlink" && <div className="grid min-h-72 place-content-center px-6 text-center"><h4 className="font-medium">{t("review:symlink")}</h4><p className="font-mono text-sm text-muted-foreground">{fileChange.before ?? t("review:none")} → {fileChange.after ?? t("review:none")}</p></div>}
              {fileChange.contentKind === "text" && <DiffEditor theme={editorTheme} height="calc(100% - 2.5rem)" language="plaintext" original={fileChange.before ?? ""} modified={fileChange.after ?? ""} options={{ readOnly: true, renderSideBySide: !narrowReview && diffMode === "side-by-side", ignoreTrimWhitespace: false }} />}
            </section>}
          </ResizablePanel>
        </ResizablePanelGroup>}
    </section>}
    </>}
  </main>;
}

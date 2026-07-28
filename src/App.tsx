import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DiffEditor } from "@monaco-editor/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useTranslation } from "react-i18next";
import { Button, buttonVariants } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useSystemTheme } from "@/lib/use-system-theme";
import { HomePage } from "@/components/home-page";
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
type PullRequestMetadata = {
  owner: string;
  repository: string;
  number: number;
  title: string;
  description: string;
  author: string;
  state: "open" | "closed" | "merged";
  draft: boolean;
  url: string;
  baseRef: string;
  headRef: string;
  synchronizedAt: number;
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
/** 首页与 Repository Entry 页面共享的持久化 Review 列表投影。 */
type ReviewSummary = {
  reviewId: string;
  repositoryEntryId: number;
  repositoryPath: string;
  baseRef: string;
  headRef: string;
  lastOpenedAt: number;
};
type PullRequestRevision = { ref: string; sha: string };
type DiscoveredPullRequest = {
  number: number;
  title: string;
  description: string;
  author: string;
  state: string;
  draft: boolean;
  url: string;
  base: PullRequestRevision;
  head: PullRequestRevision;
};
type PullRequestGroup = {
  repository: { owner: string; name: string };
  remoteNames: string[];
  pullRequests: DiscoveredPullRequest[];
  error: string | null;
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
/// Repository Entry 下无需 GitHub 在线状态即可展示的 durable Review。
type SavedReviewSummary = {
  reviewId: string;
  baseRevision: string;
  headRevision: string;
  pullRequest?: PullRequestMetadata;
};

type DirectoryNode = {
  name: string;
  directories: DirectoryNode[];
  files: FileChangeSummary[];
};

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
    directory.directories.sort((left, right) => left.name.localeCompare(right.name));
    directory.files.sort((left, right) => left.path.localeCompare(right.path));
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
  const { t } = useTranslation();
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
  const [refs, setRefs] = useState<string[]>([]);
  const [baseRef, setBaseRef] = useState("");
  const [headRef, setHeadRef] = useState("");
  const [snapshot, setSnapshot] = useState<ReviewSnapshot>();
  const [selectedPullRequest, setSelectedPullRequest] = useState<SelectedPullRequest>();
  const [reviewUpdate, setReviewUpdate] = useState<ReviewUpdate>();
  const [branchUpdate, setBranchUpdate] = useState<BranchReviewUpdate>();
  const [fileChange, setFileChange] = useState<FileChangeContent>();
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [authStatus, setAuthStatus] = useState<GitHubAuthStatus>();
  const [deviceAuthorization, setDeviceAuthorization] = useState<DeviceAuthorization>();
  const [authorizing, setAuthorizing] = useState(false);
  const [copiedValue, setCopiedValue] = useState<"device-code" | "authorization-url">();
  const activeReview = useRef<ActiveReview>();
  const activeGitOperationId = useRef<string>();
  const nextGitOperationId = useRef(0);

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

  function replaceSnapshot(nextSnapshot?: ReviewSnapshot) {
    const previousReview = activeReview.current;
    const nextReview = nextSnapshot && { reviewId: nextSnapshot.reviewId };
    activeReview.current = nextReview;
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
    setPullRequestGroups([]);
    setDiscoveryLoading(true);
    try {
      setPullRequestGroups(await invoke<PullRequestGroup[]>("discover_github_pull_requests", {
        repositoryEntryId: entry.id,
      }));
    } catch (reason) {
      setError(String(reason));
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
    void loadPullRequestGroups(opened);
    setRefs([]);
    setBaseRef("");
    setHeadRef("");
    replaceSnapshot(undefined);
    setFileChange(undefined);
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
    setLoading(true);
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
    } finally {
      setLoading(false);
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
    setLoading(true);
    try {
      if (selectedRepository?.id !== summary.repositoryEntryId) {
        const entry = repositoryEntries.find((candidate) => candidate.id === summary.repositoryEntryId);
        if (!entry) throw new Error("Review 所属 Repository Entry 不存在。");
        const result = await openRepository(entry);
        if (result !== "entered") {
          if (result === "trust-required") setPendingReview(summary);
          return;
        }
      }
      await reopenResolvedReview(summary);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoading(false);
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
    setLoading(true);
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
      setFileChange(undefined);
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }

  /** 重新查询 GitHub metadata 并检查候选 revisions，不改变当前 Snapshot。 */
  async function checkPullRequestUpdate() {
    if (!selectedRepository || !selectedPullRequest || !snapshot) return;
    setError("");
    setLoading(true);
    try {
      const groups = await invoke<PullRequestGroup[]>("discover_github_pull_requests", {
        repositoryEntryId: selectedRepository.id,
      });
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
      setReviewUpdate(await invoke<ReviewUpdate>("check_pull_request_review_update", {
        reviewId: snapshot.reviewId,
        repository: group!.repository,
        pullRequest: current,
      }));
    } catch (reason) {
      setError(String(reason).replace(/^Error: /, ""));
    } finally {
      setLoading(false);
    }
  }

  /** 仅在用户明确确认后刷新 Snapshot、File Changes 与 Review Workspace。 */
  async function refreshPullRequestReview() {
    if (!selectedPullRequest || !snapshot) return;
    setError("");
    setLoading(true);
    try {
      replaceSnapshot(await invoke<ReviewSnapshot>("refresh_pull_request_review", {
        reviewId: snapshot.reviewId,
        repository: selectedPullRequest.repository,
        pullRequest: selectedPullRequest.pullRequest,
      }));
      setReviewUpdate(undefined);
      setFileChange(undefined);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoading(false);
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

  async function loadRefs() {
    setError("");
    try {
      const loadedRefs = await invokeLocalGit<string[]>("list_local_refs", {
        repositoryEntryId: selectedRepository?.id,
      });
      setRefs(loadedRefs);
      replaceSnapshot(undefined);
      setFileChange(undefined);
      setBaseRef(loadedRefs[0] ?? "");
      setHeadRef(loadedRefs[1] ?? loadedRefs[0] ?? "");
    } catch (reason) {
      setRefs([]);
      replaceSnapshot(undefined);
      setFileChange(undefined);
      setError(String(reason));
    }
  }

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

  /** 只检查 Branch Selection 的 moving refs，不改变正在展示的 Snapshot。 */
  async function checkBranchReviewUpdates() {
    if (!snapshot) return;
    setError("");
    try {
      setBranchUpdate(await invokeLocalGit<BranchReviewUpdate>("check_branch_review_updates", {
        reviewId: snapshot.reviewId,
      }));
    } catch (reason) {
      setBranchUpdate(undefined);
      setError(String(reason));
    }
  }

  /** 在用户明确确认后刷新 Snapshot、File Changes 与 Review Workspace。 */
  async function refreshBranchReview() {
    if (!snapshot) return;
    setError("");
    try {
      replaceSnapshot(await invokeLocalGit<ReviewSnapshot>("refresh_branch_review", {
        reviewId: snapshot.reviewId,
        confirmation: branchUpdate,
      }));
      setFileChange(undefined);
    } catch (reason) {
      setError(String(reason));
    }
  }

  function renderDirectory(directory: DirectoryNode) {
    return <>
      {directory.directories.map((child) => <li key={child.name} role="treeitem" aria-label={child.name} aria-expanded="true">
        <span>{child.name}</span>
        <ul role="group">{renderDirectory(child)}</ul>
      </li>)}
      {directory.files.map((change) => <li key={change.path} role="none">
        <Button variant="ghost" type="button" role="treeitem" onClick={() => openFileChange(change.path)}>
          <span>{change.oldPath ? `${change.oldPath} → ${change.path}` : change.path.split("/").at(-1)}</span> <small>{change.status}</small>
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
    {selectedRepository && <article aria-label="Repository Entry">
      <Button variant="ghost" onClick={leaveRepository}>返回 Repository Entries</Button>
      <h2>{selectedRepository.canonicalPath.split(/[\\/]/).filter(Boolean).at(-1)}</h2>
      <p>{selectedRepository.canonicalPath}</p>
      <section aria-label="Repository Reviews">
        <h3>Reviews</h3>
        {repositoryReviews.length === 0
          ? <p>尚无 Review</p>
          : <ul>{repositoryReviews.map((review) => <li key={review.reviewId}>
            <Button variant="outline" onClick={() => void reopenReview(review)}>
              {review.baseRef.split("/").at(-1)} → {review.headRef.split("/").at(-1)}
            </Button>
          </li>)}</ul>}
      </section>
      <section aria-label="GitHub Pull Requests">
        <div className="flex flex-wrap items-center gap-3">
          <h3>GitHub Pull Requests</h3>
          <Button
            variant="outline"
            disabled={discoveryLoading}
            onClick={() => void loadPullRequestGroups(selectedRepository)}
          >
            刷新 Pull Requests
          </Button>
        </div>
        {discoveryLoading && <p>正在发现 Pull Requests…</p>}
        {!discoveryLoading && pullRequestGroups.length === 0 && <p>没有发现 GitHub remote</p>}
        {pullRequestGroups.map((group) => {
          const identity = `${group.repository.owner}/${group.repository.name}`;
          return <article key={identity} aria-label={identity}>
            <h4>{identity}</h4>
            <p>remotes: {group.remoteNames.join(", ")}</p>
            {group.error && <p role="status">{group.error}</p>}
            {!group.error && group.pullRequests.length === 0 && <p>没有可访问的 Pull Request</p>}
            {group.pullRequests.length > 0 && <ul>
              {group.pullRequests.map((pullRequest) => <li key={pullRequest.number}>
                <Button
                  variant="outline"
                  disabled={loading}
                  onClick={() => void openDiscoveredPullRequest(group.repository, pullRequest)}
                >
                  #{pullRequest.number} {pullRequest.title}
                  {pullRequest.draft ? "（草稿）" : ""}
                </Button>
                <p>{pullRequest.author} · {pullRequest.state} · {pullRequest.base.ref} → {pullRequest.head.ref}</p>
              </li>)}
            </ul>}
          </article>;
        })}
      </section>
      <section aria-label="保存的 Reviews">
        <h3>保存的 Reviews</h3>
        {savedReviews.length === 0
          ? <p>尚无保存的 Review</p>
          : <ul>{savedReviews.map((review) => <li key={review.reviewId}>
            <Button variant="outline" disabled={loading} onClick={() => void openSavedReview(review)}>
              {review.pullRequest
                ? `#${review.pullRequest.number} ${review.pullRequest.title}`
                : `${review.baseRevision.slice(0, 8)} → ${review.headRevision.slice(0, 8)}`}
            </Button>
          </li>)}</ul>}
      </section>
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
    <section aria-label="打开 GitHub Pull Request" className="flex flex-wrap items-end gap-3">
      {!authStatus && <Button variant="outline" onClick={connectGitHub}>连接 GitHub</Button>}
      {authStatus?.signedIn && <Button variant="outline" onClick={signOutGitHub}>退出 GitHub</Button>}
    </section>
    {authStatus && !authStatus.signedIn && <section aria-label="GitHub 授权">
      {!authStatus.configured
        ? <p>此构建未配置 GitHub App；公开 Repository 的 Pull Request 仍可匿名发现。</p>
        : <>
          <p>第 1 步：在 GitHub 中安装 Narrant，并为目标仓库授予只读访问；组织仓库可能需要管理员审批。</p>
          {authStatus.appInstallUrl && <Button variant="outline" onClick={() => openUrl(authStatus.appInstallUrl!)}>重新选择仓库或申请组织审批</Button>}
          <p>第 2 步：安装完成后返回此处，继续授权当前 GitHub 账号。</p>
          <Button disabled={authorizing} onClick={startGitHubAuthorization}>我已安装，继续</Button>
          {deviceAuthorization && <div>
            <p>设备码：<strong>{deviceAuthorization.userCode}</strong></p>
            <Button variant="outline" type="button" onClick={() => void copyAuthorizationValue(deviceAuthorization.userCode, "device-code")}>
              {copiedValue === "device-code" ? "设备码已复制" : "复制设备码"}
            </Button>
            <p>授权 URL：<a href={deviceAuthorization.verificationUri} onClick={(event) => { event.preventDefault(); void openUrl(deviceAuthorization.verificationUri); }}>{deviceAuthorization.verificationUri}</a></p>
            <div className="flex flex-wrap gap-3">
              <Button variant="outline" type="button" onClick={() => void copyAuthorizationValue(deviceAuthorization.verificationUri, "authorization-url")}>
                {copiedValue === "authorization-url" ? "链接已复制" : "复制链接"}
              </Button>
              <Button variant="outline" type="button" onClick={() => void openUrl(deviceAuthorization.verificationUri)}>打开 GitHub</Button>
            </div>
            <p>仅使用 Narrant 当前生成的设备码，并在 GitHub 页面核对应用名称。不要输入他人发送的验证码。</p>
            <Button disabled={authorizing} onClick={completeGitHubAuthorization}>我已在 GitHub 完成授权</Button>
          </div>}
        </>}
    </section>}
    <p>本地分支比较</p>
    <section aria-label="选择本地仓库" className="flex flex-wrap items-end gap-3">
      <label>本地仓库路径<Input aria-label="本地仓库路径" value={repositoryPath} readOnly /></label>
      <Button variant="outline" onClick={loadRefs}>读取分支</Button>
      <label>Base Ref<Select value={baseRef} onValueChange={(value) => setBaseRef(value ?? "")}><SelectTrigger aria-label="Base Ref"><SelectValue placeholder="选择 Base Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
      <label>Head Ref<Select value={headRef} onValueChange={(value) => setHeadRef(value ?? "")}><SelectTrigger aria-label="Head Ref"><SelectValue placeholder="选择 Head Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
      <Button disabled={!baseRef || !headRef || (catalogRuntimeEnabled && !workspaceTrusted)} onClick={openComparison}>打开 Review Snapshot</Button>
    </section>
    {error && <ErrorMessage message={error} />}
    {snapshot && <Button variant="outline" onClick={closeReviewView}>关闭 Review</Button>}
    {snapshot?.pullRequest && <article aria-label="Pull Request 概览">
      <h2>{snapshot.pullRequest.title}</h2>
      <p>#{snapshot.pullRequest.number} · {snapshot.pullRequest.author} · {snapshot.pullRequest.draft ? "草稿 · " : ""}{{ open: "开放", closed: "已关闭", merged: "已合并" }[snapshot.pullRequest.state ?? "open"]}</p>
      <p>{snapshot.baseRef} @ {snapshot.baseRevision} → {snapshot.headRef} @ {snapshot.headRevision}</p>
      <div>
        <Button variant="outline" disabled={loading} onClick={() => void checkPullRequestUpdate()}>
          检查 Pull Request 更新
        </Button>
        {reviewUpdate && (reviewUpdate.updateAvailable
          ? <div role="status">
            <p>发现新的 Pull Request revisions；当前 Snapshot 保持不变。</p>
            <Button disabled={loading} onClick={() => void refreshPullRequestReview()}>刷新 Review</Button>
          </div>
          : <p role="status">当前已是最新 Snapshot</p>)}
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
      <Button variant="outline" onClick={checkBranchReviewUpdates}>检查更新</Button>
      {branchUpdate && !branchUpdate.changed && <p role="status">当前已是最新版本</p>}
      {branchUpdate?.changed && <div role="status">
        <p>发现新版本：{branchUpdate.currentHeadRevision} → {branchUpdate.latestHeadRevision}</p>
        <Button onClick={refreshBranchReview}>确认刷新到新版本</Button>
      </div>}
    </section>}
    {snapshot && <div className="grid grid-cols-[minmax(16rem,24rem)_1fr] gap-4">
      <nav aria-label="文件树">
        <ul role="tree" aria-label="文件树">
          {renderDirectory(buildDirectoryTree(snapshot.fileChanges))}
        </ul>
      </nav>
      {snapshot.fileChanges.length === 0 && <p>没有 File Change</p>}
      {fileChange && <section aria-label="File Change" data-before={import.meta.env.VITE_WDIO ? fileChange.before ?? "" : undefined} data-after={import.meta.env.VITE_WDIO ? fileChange.after ?? "" : undefined}>
        <h2>{fileChange.path} <small>{fileChange.status}</small></h2>
        {fileChange.contentKind === "binary"
          ? <p>二进制文件（变更前：{fileChange.beforeSize === null ? "无" : `${fileChange.beforeSize} 字节`}；变更后：{fileChange.afterSize === null ? "无" : `${fileChange.afterSize} 字节`}）</p>
          : <>
            {fileChange.contentKind === "submodule" && <p>Submodule commit</p>}
            {fileChange.contentKind === "symlink" && <p>Symlink 目标</p>}
            <DiffEditor theme={editorTheme} height="500px" language="plaintext" original={fileChange.before ?? ""} modified={fileChange.after ?? ""} options={{ readOnly: true, renderSideBySide: true, ignoreTrimWhitespace: false }} />
          </>}
      </section>}
    </div>}
    </>}
  </main>;
}

import { StrictMode, type ReactNode, useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";

import "@/index.css";
import { BranchReviewSelector } from "@/components/branch-review-selector";
import { FileChangeLoading } from "@/components/file-change-loading";
import { GitHubIdentityEntry } from "@/components/github-identity-entry";
import { GitHubAuthorizationControls } from "@/components/github-authorization-controls";
import { HomePage } from "@/components/home-page";
import { RepositoryOverview } from "@/components/repository-overview";
import { RepositoryTrustDialog } from "@/components/repository-trust-dialog";
import { ReviewReader, type DiffMode, type FileChangeContent, type ReviewReaderSnapshot } from "@/components/review-reader";
import type { ReviewReaderBinding } from "@/review/use-review-lifecycle";
import type { BabysitBinding, BabysitFileExplanations } from "@/babysit/types";
import { TooltipProvider } from "@/components/ui/tooltip";
import { i18n } from "@/lib/i18n";
import type { RepositoryReviewSummary } from "@/lib/review-types";
import type { GitHubIdentityLifecycleBinding } from "@/github/use-github-identity-lifecycle";
import type { BranchSelectionContinuationBinding } from "@/review/use-branch-selection-continuation";
import { ApplicationSettingsDestination } from "@/components/settings-page";
import type { ApplicationLanguage, ApplicationSettings, ApplicationSettingsApplication, BabysitSettings, CustomProviderConnection, ThemePreference } from "@/settings/application-settings";
import type { ApplicationUpdateApplication, ApplicationUpdateSnapshot } from "@/application-update/application-update";
import { useApplicationUpdate, type ApplicationUpdateBinding } from "@/application-update/use-application-update";
import { ApplicationUpdateAttention } from "@/components/application-update/application-update-attention";
import { ApplicationUpdateSurface } from "@/components/application-update/application-update-surface";
import type { SettingsSection } from "@/settings/settings-section";
import { resolvePreviewScenarioKey } from "@/dev/preview-routing";

/** 描述一个可以直接在浏览器中验收的组件场景。 */
type PreviewScenario = {
  label: string;
  render: () => ReactNode;
};

/** 描述一个组件及其可切换的代表性预览场景。 */
type PreviewDefinition = {
  label: string;
  scenarios: Record<string, PreviewScenario>;
};

/** Settings Preview 中 Custom 连接的可交互状态集合。 */
type CustomPreviewState = "empty" | "configured" | "invalid" | "saving" | "failed" | "revealed";

/** Application Update Preview 允许直接构造的 provider-neutral 状态。 */
type UpdatePreviewKind = ApplicationUpdateSnapshot["state"]["kind"];

/** 构造不含 URL、路径、签名或 target 的代表性更新快照。 */
function updatePreviewSnapshot(kind: UpdatePreviewKind, options: { longNotes?: boolean; knownProgress?: boolean } = {}): ApplicationUpdateSnapshot {
  const candidate = ["available", "downloading", "staged", "failed"].includes(kind) ? {
    version: "2.4.0",
    releaseNotes: options.longNotes
      ? `# 2.4.0\n\n${"这是一段用于验证窄窗口换行与长版本说明滚动的内容。".repeat(12)}\n\n## 改进\n\n- 支持安全暂存更新\n- 保留当前 Review 工作\n\n| 项目 | 结果 |\n| --- | --- |\n| 安全验证 | 通过 |\n| 原始 HTML | <script>blocked</script> |\n\n[安全链接](https://example.com/release) [危险链接](javascript:alert(1))`
      : "## 此版本的改进\n\n- 更可靠的应用更新\n- 更清晰的下载进度",
  } : null;
  const state: ApplicationUpdateSnapshot["state"] = kind === "downloading"
    ? { kind, progress: options.knownProgress
      ? { kind: "known_total", downloadedBytes: 7_340_032, totalBytes: 12_582_912 }
      : { kind: "unknown_total", downloadedBytes: 3_145_728 } }
    : { kind };
  return {
    sequence: 12,
    currentVersion: "2.3.1",
    state,
    automaticDownload: true,
    candidate,
    failure: kind === "failed" ? { operation: "download", category: "network", retryable: true, message: "redacted" } : null,
    recoveryWarning: null,
    preferenceWarning: null,
  };
}

/** 用窄应用端口驱动真实更新组件，供浏览器检查所有交互状态。 */
function ApplicationUpdatePreview({
  kind,
  longNotes = false,
  knownProgress = false,
  settings = false,
  longIdentity = false,
}: {
  kind: UpdatePreviewKind;
  longNotes?: boolean;
  knownProgress?: boolean;
  settings?: boolean;
  longIdentity?: boolean;
}) {
  const initial = useMemo(() => updatePreviewSnapshot(kind, { longNotes, knownProgress }), [kind, knownProgress, longNotes]);
  const application = useMemo<ApplicationUpdateApplication>(() => {
    let current = initial;
    const listeners = new Set<(snapshot: ApplicationUpdateSnapshot) => void>();
    const publish = (next: ApplicationUpdateSnapshot) => {
      current = next;
      listeners.forEach((listener) => listener(next));
      return Promise.resolve(next);
    };
    return {
      snapshot: async () => current,
      subscribe: async (listener) => { listeners.add(listener); return () => listeners.delete(listener); },
      manualCheck: () => publish({ ...current, sequence: current.sequence + 1, state: { kind: "checking" }, failure: null }),
      setAutomaticDownload: (enabled) => publish({ ...current, sequence: current.sequence + 1, automaticDownload: enabled, preferenceWarning: null }),
      retry: () => publish({ ...current, sequence: current.sequence + 1, state: { kind: "checking" }, failure: null }),
      restart: async () => current,
    };
  }, [initial]);
  const update = useApplicationUpdate(application);
  const updateSnapshot = update.snapshot;
  const openUpdateSurface = update.openSurface;
  useEffect(() => {
    if (!settings && updateSnapshot) openUpdateSurface();
  }, [openUpdateSurface, settings, updateSnapshot]);
  if (settings) return <SettingsPreview applicationUpdate={update} initialSection="application-update" />;
  return <>
    <HomePage
      reviews={[]}
      onOpenReview={() => undefined}
      onOpenFolder={() => undefined}
      githubIdentityEntry={<GitHubIdentityEntry
        className="h-auto min-h-12 w-full justify-start py-2"
        signedIn
        identity={{ id: 42, login: longIdentity ? "octocat-with-an-extraordinarily-long-github-identity-that-must-truncate" : "octocat", name: longIdentity ? "Monalisa Octocat With An Extraordinarily Long Display Name" : "Monalisa", avatarPath: null }}
        onConnect={() => undefined}
        onSignOut={() => undefined}
      />}
      applicationUpdateAttention={<ApplicationUpdateAttention snapshot={update.snapshot} onOpen={update.openSurface} />}
    />
    <ApplicationUpdateSurface update={update} />
  </>;
}

/** 用公开端口提供可交互的主题、语言与持久化失败预览。 */
function SettingsPreview({
  saveFails = false,
  language: initialLanguage = "zh-CN",
  longCodexPath = false,
  babysitSection = false,
  customState,
  githubState,
  modelState = "idle",
  applicationUpdate,
  initialSection,
}: {
  saveFails?: boolean;
  language?: ApplicationLanguage;
  longCodexPath?: boolean;
  babysitSection?: boolean;
  customState?: CustomPreviewState;
  githubState?: "signed-in" | "confirm" | "failed";
  modelState?: "idle" | "loading" | "empty" | "error";
  applicationUpdate?: ApplicationUpdateBinding;
  initialSection?: SettingsSection;
}) {
  const [savedTheme, setSavedTheme] = useState<ThemePreference>("system");
  const [savedLanguage, setSavedLanguage] = useState<ApplicationLanguage>(initialLanguage);
  const [codexExecutable, setCodexExecutable] = useState<string | null>(longCodexPath
    ? `C:\\${"very-long-directory-name\\".repeat(10)}codex.exe`
    : null);
  const [customConnection, setCustomConnection] = useState<CustomProviderConnection>(customState === "empty"
    ? { protocol: "openai_chat_completions", baseUrl: "", apiKey: "", model: "", effort: "provider_default" }
    : customState === "invalid"
      ? { protocol: "openai_chat_completions", baseUrl: "   ", apiKey: "sk-preview-key", model: "   ", effort: "provider_default" }
      : { protocol: "openai_chat_completions", baseUrl: "https://api.example.test/v1", apiKey: "sk-preview-key", model: "gpt-5-mini", effort: "medium" });
  const [babysit, setBabysit] = useState<BabysitSettings>({
    provider: customState ? "custom" : babysitSection ? "claude-code" : "codex",
    repositoryExploration: false,
    allNonEmptyLines: false,
    codex: { executable: codexExecutable, model: null, reasoningEffort: null },
    claudeCode: { executable: babysitSection ? "/opt/homebrew/bin/claude" : null, model: babysitSection ? "sonnet" : null, reasoningEffort: babysitSection ? "low" : null },
    custom: { protocol: customConnection.protocol, baseUrl: customConnection.baseUrl, model: customConnection.model, effort: customConnection.effort },
  });
  const snapshot = (theme: ThemePreference, value = babysit, language = savedLanguage): ApplicationSettings => ({ version: 1, theme, language, babysit: value });
  const application: ApplicationSettingsApplication = {
    load: async () => snapshot(savedTheme),
    updateTheme: async (theme) => {
      if (saveFails) throw new Error("预览中的 settings.json 无法写入");
      setSavedTheme(theme);
      return snapshot(theme);
    },
    updateLanguage: async (language) => {
      if (saveFails) throw new Error("预览中的 settings.json 无法写入");
      setSavedLanguage(language);
      return snapshot(savedTheme, babysit, language);
    },
    listBabysitModels: async () => {
      if (modelState === "loading") return new Promise(() => undefined);
      if (modelState === "error") throw { code: "app_server_unavailable", detail: "App Server 暂时无法启动" };
      if (modelState === "empty") return { provider: "codex", revision: "preview-catalog", models: [] };
      return { provider: "codex", revision: "preview-catalog", models: [
        { id: "gpt-5.4", name: "GPT-5.4", isDefault: true, supportedReasoningEfforts: ["low", "medium", "high"], defaultReasoningEffort: "high" },
        { id: "gpt-5.4-mini", name: "GPT-5.4 Mini", isDefault: false, supportedReasoningEfforts: ["low", "medium"], defaultReasoningEffort: "medium" },
      ] };
    },
    updateBabysitProvider: async (provider) => {
      const next = { ...babysit, provider }; setBabysit(next); return snapshot(savedTheme, next);
    },
    updateBabysitRepositoryExploration: async (repositoryExploration) => {
      if (saveFails) throw new Error("预览中的 settings.json 无法写入");
      const next = { ...babysit, repositoryExploration }; setBabysit(next); return snapshot(savedTheme, next);
    },
    updateBabysitAllNonEmptyLines: async (allNonEmptyLines) => {
      if (saveFails) throw new Error("预览中的 settings.json 无法写入");
      const next = { ...babysit, allNonEmptyLines }; setBabysit(next); return snapshot(savedTheme, next);
    },
    chooseBabysitExecutable: async (provider, preference) => {
      if (saveFails) throw new Error("预览中的 provider 版本验证失败");
      const executable = provider === "codex" ? "C:\\Program Files\\Codex\\codex.exe" : "C:\\Program Files\\Claude\\claude.exe";
      const key = provider === "codex" ? "codex" : "claudeCode";
      const next = { ...babysit, [key]: { ...preference, executable } }; setCodexExecutable(next.codex.executable); setBabysit(next); return snapshot(savedTheme, next);
    },
    updateBabysitPreferences: async (provider, preference) => {
      const key = provider === "codex" ? "codex" : "claudeCode";
      const next = { ...babysit, [key]: preference }; setCodexExecutable(next.codex.executable); setBabysit(next); return snapshot(savedTheme, next);
    },
    loadCustomConnection: async () => customConnection,
    saveCustomConnection: async (connection) => {
      if (customState === "saving") return new Promise<ApplicationSettings>(() => undefined);
      if (customState === "failed") throw new Error("预览中的 Custom 连接无法保存");
      const next = { ...babysit, custom: { protocol: connection.protocol, baseUrl: connection.baseUrl, model: connection.model, effort: connection.effort } };
      setCustomConnection(connection);
      setBabysit(next);
      return snapshot(savedTheme, next);
    },
  };
  const githubIdentity: GitHubIdentityLifecycleBinding | undefined = githubState ? {
    status: { configured: true, signedIn: true, appInstallUrl: null, identity: { id: 42, login: "octocat", name: "Monalisa Octocat", avatarPath: null } },
    error: githubState === "failed" ? { scope: "github-identity", operation: "sign-out", detail: "凭据存储暂时不可用，请重试。" } : undefined,
    transition: { kind: "stable" },
    beginAuthorization: async () => ({ kind: "failed" }),
    completeAuthorization: async () => ({ kind: "failed" }),
    signOut: async () => ({ kind: githubState === "failed" ? "failed" : "signed-out" }),
    retry: async () => undefined,
  } : undefined;
  return <ApplicationSettingsDestination
    application={application}
    githubIdentity={githubIdentity}
    applicationUpdate={applicationUpdate}
    initialSection={initialSection ?? (githubState ? "github" : longCodexPath || babysitSection || customState || modelState !== "idle" ? "babysit" : "appearance")}
    initialCustomApiKeyVisible={customState === "revealed"}
    autoSaveCustomConnection={customState === "saving"}
    initialGitHubSignOutConfirmation={githubState === "confirm"}
    onBack={() => undefined}
  />;
}

/** 用与真实 App 相同的组件预览展开中的 Repository Trust 弹层。 */
function RepositoryTrustPreview({ granting = false }: { granting?: boolean }) {
  const [open, setOpen] = useState(true);
  return <div className="min-h-[32rem] rounded-xl border bg-card/40 p-6">
    <h2 className="font-heading text-lg font-semibold">Repository Overview</h2>
    <p className="mt-2 text-sm text-muted-foreground">选择本地仓库后，需要先确认工作区信任。</p>
    <RepositoryTrustDialog
      open={open}
      canonicalPath="/Users/example/projects/a-repository-with-a-long-readable-name"
      granting={granting}
      onCancel={() => setOpen(false)}
      onConfirm={() => undefined}
    />
  </div>;
}

const defaultRefs = [
  "refs/heads/main",
  "refs/heads/release",
  "refs/heads/codex/issue-11-system-theme",
  "refs/heads/codex/issue-21-repository-entries",
  "refs/remotes/origin/main",
  "refs/remotes/origin/release",
];

const longHeadRef = "refs/heads/codex/frontend-product-experience-spec-with-a-very-long-suffix";

/** 用可交互的本地状态承载一个 Branch Selection 预览。 */
function BranchSelectorPreview({
  refs = defaultRefs,
  initialHead = "refs/heads/release",
  initialBase = "refs/remotes/origin/main",
  currentRef = "refs/heads/release",
  action = "start",
  continuationState,
  opening = false,
}: {
  refs?: string[];
  initialHead?: string;
  initialBase?: string;
  currentRef?: string;
  action?: Extract<BranchSelectionContinuationBinding["state"], { kind: "ready" }>["action"];
  continuationState?: BranchSelectionContinuationBinding["state"];
  opening?: boolean;
}) {
  const [headRef, setHeadRef] = useState(initialHead);
  const [baseRef, setBaseRef] = useState(initialBase);
  const continuation: BranchSelectionContinuationBinding = {
    state: continuationState ?? { kind: "ready", action },
    submit: async () => ({ kind: "ignored" }),
  };

  return <BranchReviewSelector
    refs={refs}
    currentRef={currentRef}
    defaultRef="refs/remotes/origin/main"
    headRef={headRef}
    baseRef={baseRef}
    continuation={continuation}
    opening={opening}
    reviewNavigationBusy={opening}
    onHeadChange={setHeadRef}
    onBaseChange={setBaseRef}
    onReload={() => undefined}
  />;
}

const reviewHistory: RepositoryReviewSummary[] = [
  {
    reviewId: "review-preview-1",
    selection: { kind: "branch", baseRef: "refs/heads/main", headRef: "refs/heads/codex/review-history-navigation" },
    currentSnapshot: { baseRevision: "base-1", comparisonBase: "merge-base-1", headRevision: "head-1" },
    lastOpenedAt: Date.now(),
  },
  {
    reviewId: "review-preview-2",
    selection: { kind: "branch", baseRef: "refs/remotes/upstream/release-with-a-long-name", headRef: "refs/heads/feature/a-review-with-an-extraordinarily-long-branch-name" },
    currentSnapshot: { baseRevision: "base-2", comparisonBase: "merge-base-2", headRevision: "head-2" },
    lastOpenedAt: Date.now() - 60_000,
  },
];

/** 用本地状态预览审核历史的整行导航与打开反馈。 */
function ReviewHistoryPreview({ initiallyOpening = false }: { initiallyOpening?: boolean }) {
  const [openingReviewKey, setOpeningReviewKey] = useState(
    initiallyOpening ? "review:review-preview-1" : undefined,
  );
  return <RepositoryOverview
    repositoryName="narrant"
    repositoryPath="/work/narrant"
    reviews={reviewHistory}
    pullRequestGroups={[]}
    discoveryLoading={false}
    githubConnected
    localBranches={null}
    initialTab="history"
    openingReviewKey={openingReviewKey}
    reviewNavigationBusy={Boolean(openingReviewKey)}
    onRetryDiscovery={() => undefined}
    onConnectGitHub={() => undefined}
    onOpenPullRequest={() => undefined}
    onOpenReview={(review) => setOpeningReviewKey(`review:${review.reviewId}`)}
  />;
}

/** 预览全局 GitHub Identity 入口的正常与可恢复错误状态。 */
function GitHubIdentityPreview({ error = false, signedIn = false }: { error?: boolean; signedIn?: boolean }) {
  return <div className="w-64 rounded-xl border bg-background p-3 shadow-sm">
    <GitHubIdentityEntry
      className="h-auto min-h-12 w-full justify-start py-2"
      signedIn={signedIn}
      identity={signedIn ? { id: 42, login: "octocat", name: "Monalisa", avatarPath: null } : undefined}
      error={error ? "credential store unavailable" : undefined}
      onConnect={() => undefined}
      onSignOut={() => undefined}
      onRetry={() => undefined}
    />
  </div>;
}

/** 通过 lifecycle interface 的代表性数据预览 GitHub 授权向导。 */
function GitHubAuthorizationPreview({ deviceFlow = false }: { deviceFlow?: boolean }) {
  const authorization = deviceFlow ? {
    authorizationId: "preview-authorization",
    userCode: "ABCD-EFGH",
    verificationUri: "https://github.com/login/device",
    expiresIn: 900,
    interval: 5,
  } : undefined;
  const identity: GitHubIdentityLifecycleBinding = {
    status: {
      configured: true,
      signedIn: false,
      appInstallUrl: "https://github.com/apps/narrant/installations/new",
      identity: null,
    },
    authorization,
    transition: { kind: "stable" },
    beginAuthorization: async () => authorization
      ? { kind: "started", authorization }
      : { kind: "failed" },
    completeAuthorization: async () => ({ kind: "authorized" }),
    signOut: async () => ({ kind: "signed-out" }),
    retry: async () => undefined,
  };
  return <div className="mx-auto w-full max-w-lg rounded-xl border bg-background p-6 shadow-sm">
    <GitHubAuthorizationControls identity={identity} onConnect={() => undefined} />
  </div>;
}

const readerSnapshot: ReviewReaderSnapshot = {
  reviewId: "review-reader-preview",
  snapshotId: "snapshot-reader-preview",
  baseRef: "refs/heads/codex/frontend-product-experience-spec",
  baseRevision: "c530a07d6a0bd935a5aa7a62e34f19e2f3ea2bdd",
  comparisonBase: "c530a07d6a0bd935a5aa7a62e34f19e2f3ea2bdd",
  headRef: "refs/heads/codex/issue-45-product-experience-with-a-very-long-branch-name",
  headRevision: "274aaa2f9b3b0d3e7363d09fa21dfe76782d25cf",
  fileChanges: [
    { path: ".github/workflows/ci.yml", oldPath: null, status: "modified" },
    { path: "Dockerfile", oldPath: null, status: "added" },
    { path: "docs/review-reader.md", oldPath: null, status: "added" },
    { path: "fixtures/unknown.fixture", oldPath: null, status: "modified" },
    { path: "src/components/review-reader.tsx", oldPath: null, status: "modified" },
    { path: "src/components/legacy-reader.tsx", oldPath: null, status: "deleted" },
    { path: "src/lib/review-navigation.ts", oldPath: "src/lib/diff-navigation.ts", status: "renamed" },
    { path: "tests/review-reader.test.tsx", oldPath: null, status: "modified" },
    { path: "very-long-directory-name-that-must-truncate/a-very-long-file-name-that-must-truncate-in-a-narrow-file-tree.test.ts", oldPath: null, status: "modified" },
  ],
  pullRequest: {
    owner: "narrant", repository: "desktop", number: 14, title: "Improve the Review Reader",
    description: "# Review context\n\n## User impact\n\nThe Review Reader keeps context and code changes in peer views.\n\n- Clearer information hierarchy\n- Safer Markdown rendering\n\n## Developer impact\n\nUse `ReviewView` as the shared navigation concept.\n\n> Remote images stay blocked until the reviewer opens them explicitly.\n\n```ts\nconst activeView = 'overview';\n```",
    authorUsername: "reviewer", state: "open", isDraft: false,
    webUrl: "https://github.com/narrant/desktop/pull/14", baseRef: "main",
    headRef: "feature/review-reader", synchronizedAtMillis: Date.now(),
  },
};

/** 用代表性长短路径和变更内容预览完整 Review Reader。 */
function ReviewReaderPreview({
  empty = false,
  merged = false,
  updateState = "current",
  initialView = "overview",
  shortContent = false,
  babysitKind,
}: {
  empty?: boolean;
  merged?: boolean;
  updateState?: "current" | "checking" | "failed" | "available";
  initialView?: "overview" | "file-changes";
  shortContent?: boolean;
  babysitKind?: "completed" | "generating" | "repairing" | "failed" | "claude-recovery" | "claude-signed-out" | "custom-recovery";
}) {
  const fileChanges = shortContent
    ? readerSnapshot.fileChanges.slice(0, 2)
    : readerSnapshot.fileChanges;
  const snapshot = {
    ...readerSnapshot,
    fileChanges: empty ? [] : fileChanges,
    pullRequest: merged && readerSnapshot.pullRequest
      ? { ...readerSnapshot.pullRequest, state: "merged" as const }
      : readerSnapshot.pullRequest,
  };
  const contents: Record<string, FileChangeContent> = Object.fromEntries(snapshot.fileChanges.map((change) => [
    change.path,
    {
      ...change, contentKind: "text", beforeSize: 48, afterSize: 78,
      before: shortContent
        ? "return 'old';\n"
        : "export function review() {\n  return 'old reader';\n}\n",
      after: shortContent
        ? "return 'new';\n"
        : "export function review() {\n  return 'focused review reader';\n}\n",
    },
  ]));
  const [selectedPath, setSelectedPath] = useState(snapshot.fileChanges[0]?.path);
  const [mode, setMode] = useState<DiffMode>("side-by-side");
  const [view, setView] = useState<"overview" | "file-changes">(initialView);
  const selectedContent = selectedPath ? contents[selectedPath] : undefined;
  const babysitRequest = selectedPath
    ? { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: selectedPath, detectedLanguage: "typescript" }
    : undefined;
  const babysitExplanations: BabysitFileExplanations = {
    before: selectedContent?.before ? { "1": "定义当前文件的旧实现入口。", "2": "返回修改前的阅读器结果。" } : {},
    after: selectedContent?.after ? { "1": "定义当前文件的新实现入口。", "2": "返回聚焦后的阅读器结果。" } : {},
  };
  const babysitState: BabysitBinding["state"] = !babysitKind || !babysitRequest
    ? { kind: "idle" }
    : babysitKind === "completed"
      ? { kind: "completed", request: babysitRequest, explanations: babysitExplanations }
      : babysitKind === "repairing"
        ? { kind: "repairing", request: babysitRequest, attempt: 2 }
        : babysitKind === "claude-recovery"
          ? { kind: "failed", request: babysitRequest, retryable: true, error: { code: "provider_not_found", detail: "Claude Code executable unavailable", providerId: "claude-code", recovery: "open_application_settings" } }
        : babysitKind === "claude-signed-out"
          ? { kind: "failed", request: babysitRequest, retryable: true, error: { code: "provider_signed_out", detail: "Claude Code authentication unavailable", providerId: "claude-code", recovery: "login_provider" } }
        : babysitKind === "custom-recovery"
          ? { kind: "failed", request: babysitRequest, retryable: true, error: { code: "provider_not_found", detail: "Custom provider is not configured", providerId: "custom", recovery: "open_application_settings" } }
        : babysitKind === "failed"
          ? { kind: "failed", request: babysitRequest, retryable: true, error: { code: "repair_exhausted", detail: "三轮修复后仍有行解释缺失。" } }
          : { kind: "generating", request: babysitRequest };
  const review: ReviewReaderBinding = {
    snapshot,
    transition: { kind: "stable" },
    fileChange: selectedPath
      ? { kind: "ready", path: selectedPath, content: contents[selectedPath] }
      : { kind: "empty" },
    update: updateState === "available"
      ? { kind: "available", update: { currentBaseRevision: snapshot.baseRevision, currentHeadRevision: snapshot.headRevision, availableBaseRevision: "next-base", availableHeadRevision: "next-head" } }
      : updateState === "checking" ? { kind: "checking" }
      : updateState === "failed" ? { kind: "failed", failure: { kind: "operation-failed", detail: "无法连接上游，请稍后重试。", retryable: true } }
      : { kind: "current" },
    babysit: {
      eligibility: { kind: "resolved", result: { eligible: true, reason: null } },
      state: babysitState,
      explain: () => undefined,
      regenerate: () => undefined,
      cancel: () => undefined,
      retry: () => undefined,
    },
    view,
    setView,
    diffMode: mode,
    setDiffMode: setMode,
    treeWidth: 26,
    setTreeWidth: () => undefined,
    collapsedDirectories: [],
    toggleDirectory: () => undefined,
    scrollPositions: { overview: 0, fileTree: 0, content: 0 },
    setScrollPosition: () => undefined,
    fileViewState: null,
    setFileViewState: () => undefined,
    selectFileChange: setSelectedPath,
    retryFileChange: () => undefined,
    checkForUpdate: async () => updateState === "available",
    refresh: async () => ({ kind: "refreshed", reviewId: snapshot.reviewId, snapshotId: "preview-refreshed" }),
    close: async () => ({ kind: "closed" }),
  };
  return <div className="h-[calc(100vh-8.5rem)]">
    <ReviewReader
      review={review}
      appearance={{
        narrow: window.innerWidth < 1200,
        editorTheme: document.documentElement.classList.contains("dark") ? "vs-dark" : "light",
        onOpenExternal: () => undefined,
        onOpenApplicationSettings: () => undefined,
      }}
    />
  </div>;
}

const previews: Record<string, PreviewDefinition> = {
  settings: {
    label: "应用设置",
    scenarios: {
      default: { label: "外观与分区导航", render: () => <SettingsPreview /> },
      error: { label: "主题保存失败", render: () => <SettingsPreview saveFails /> },
      "language-zh-CN": { label: "简体中文", render: () => <SettingsPreview language="zh-CN" /> },
      "language-en-US": { label: "English", render: () => <SettingsPreview language="en-US" /> },
      "language-error": { label: "语言保存失败", render: () => <SettingsPreview language="en-US" saveFails /> },
      "long-codex-path": { label: "Codex 长路径", render: () => <SettingsPreview longCodexPath /> },
      "github-signed-in": { label: "GitHub 已登录", render: () => <SettingsPreview githubState="signed-in" /> },
      "github-confirm": { label: "GitHub 退出确认", render: () => <SettingsPreview githubState="confirm" /> },
      "github-failed": { label: "GitHub 退出失败", render: () => <SettingsPreview githubState="failed" /> },
      "model-wide": { label: "模型编辑（宽窗口）", render: () => <SettingsPreview babysitSection /> },
      "model-loading": { label: "模型目录加载", render: () => <SettingsPreview modelState="loading" /> },
      "model-empty": { label: "模型目录为空", render: () => <SettingsPreview modelState="empty" /> },
      "model-error": { label: "模型目录失败", render: () => <SettingsPreview modelState="error" /> },
      "custom-empty": { label: "Custom 空配置", render: () => <SettingsPreview customState="empty" /> },
      "custom-configured": { label: "Custom 已配置", render: () => <SettingsPreview customState="configured" /> },
      "custom-invalid": { label: "Custom 本地校验失败", render: () => <SettingsPreview customState="invalid" /> },
      "custom-saving": { label: "Custom 保存中", render: () => <SettingsPreview customState="saving" /> },
      "custom-revealed": { label: "Custom 密钥已显示", render: () => <SettingsPreview customState="revealed" /> },
      "custom-failed": { label: "Custom 保存失败", render: () => <SettingsPreview customState="failed" /> },
      "application-update": { label: "应用更新设置", render: () => <ApplicationUpdatePreview kind="up_to_date" settings /> },
    },
  },
  "application-update": {
    label: "应用更新",
    scenarios: {
      available: { label: "宽窗口与短说明", render: () => <ApplicationUpdatePreview kind="available" /> },
      "narrow-long-notes": { label: "窄窗口、长身份与长说明", render: () => <ApplicationUpdatePreview kind="available" longNotes longIdentity /> },
      "downloading-determinate": { label: "确定下载进度", render: () => <ApplicationUpdatePreview kind="downloading" knownProgress /> },
      "downloading-indeterminate": { label: "不确定下载进度", render: () => <ApplicationUpdatePreview kind="downloading" /> },
      failed: { label: "可重试失败", render: () => <ApplicationUpdatePreview kind="failed" /> },
      staged: { label: "已暂存并可重启", render: () => <ApplicationUpdatePreview kind="staged" /> },
    },
  },
  "app-shell": {
    label: "应用框架",
    scenarios: {
      default: {
        label: "Windows 标题栏",
        render: () => <HomePage reviews={[]} onOpenReview={() => undefined} onOpenFolder={() => undefined} />,
      },
    },
  },
  "repository-trust": {
    label: "Repository Trust",
    scenarios: {
      default: { label: "等待确认", render: () => <RepositoryTrustPreview /> },
      granting: { label: "正在授权", render: () => <RepositoryTrustPreview granting /> },
    },
  },
  "github-authorization": {
    label: "GitHub 授权",
    scenarios: {
      install: { label: "安装 GitHub App", render: () => <GitHubAuthorizationPreview /> },
      "device-flow": { label: "Device Flow", render: () => <GitHubAuthorizationPreview deviceFlow /> },
    },
  },
  "github-identity": {
    label: "GitHub Identity",
    scenarios: {
      default: { label: "未连接", render: () => <GitHubIdentityPreview /> },
      error: { label: "状态读取失败", render: () => <GitHubIdentityPreview error /> },
      "signed-in": { label: "已连接", render: () => <GitHubIdentityPreview signedIn /> },
      "signed-in-error": { label: "退出失败", render: () => <GitHubIdentityPreview signedIn error /> },
    },
  },
  "branch-selector": {
    label: "分支选择器",
    scenarios: {
      default: {
        label: "默认状态",
        render: () => <BranchSelectorPreview />,
      },
      "long-name": {
        label: "长分支名",
        render: () => <BranchSelectorPreview
          refs={[longHeadRef, ...defaultRefs, "refs/remotes/upstream/feature/a-long-remote-tracking-branch-name"]}
          initialHead={longHeadRef}
          currentRef={longHeadRef}
        />,
      },
      "many-branches": {
        label: "大量分支",
        render: () => <BranchSelectorPreview refs={[
          ...defaultRefs,
          ...Array.from({ length: 24 }, (_, index) => `refs/heads/feature/preview-${String(index + 1).padStart(2, "0")}`),
        ]} />,
      },
      update: {
        label: "检测到更新",
        render: () => <BranchSelectorPreview action="refresh" />,
      },
      checking: {
        label: "正在检查更新",
        render: () => <BranchSelectorPreview continuationState={{ kind: "checking", showMessage: true }} />,
      },
      fallback: {
        label: "检查失败降级",
        render: () => <BranchSelectorPreview action="fallback" />,
      },
      opening: {
        label: "正在打开 Review",
        render: () => <BranchSelectorPreview action="continue" opening />,
      },
    },
  },
  "file-change-loading": {
    label: "File Change 加载",
    scenarios: {
      default: {
        label: "Diff 区域加载",
        render: () => <section className="min-h-[32rem] overflow-hidden rounded-lg border bg-background">
          <header className="border-b px-4 py-3 text-sm font-semibold">src/components/example.tsx</header>
          <FileChangeLoading label="正在读取 File Change" />
        </section>,
      },
    },
  },
  "review-history": {
    label: "审核历史",
    scenarios: {
      default: {
        label: "可点击列表",
        render: () => <ReviewHistoryPreview />,
      },
      opening: {
        label: "正在打开 Review",
        render: () => <ReviewHistoryPreview initiallyOpening />,
      },
    },
  },
  "review-reader": {
    label: "Review 阅读器",
    scenarios: {
      default: { label: "多种文件状态", render: () => <ReviewReaderPreview /> },
      "file-changes": { label: "File Changes 长列表", render: () => <ReviewReaderPreview initialView="file-changes" /> },
      "short-content": { label: "短内容 File Change", render: () => <ReviewReaderPreview initialView="file-changes" shortContent /> },
      babysit: { label: "宝宝模式完成", render: () => <ReviewReaderPreview initialView="file-changes" babysitKind="completed" /> },
      "babysit-generating": { label: "宝宝模式生成中", render: () => <ReviewReaderPreview initialView="file-changes" babysitKind="generating" /> },
      "babysit-repairing": { label: "宝宝模式修复中", render: () => <ReviewReaderPreview initialView="file-changes" babysitKind="repairing" /> },
      "babysit-failed": { label: "宝宝模式失败", render: () => <ReviewReaderPreview initialView="file-changes" babysitKind="failed" /> },
      "babysit-claude-recovery": { label: "Claude Code 恢复", render: () => <ReviewReaderPreview initialView="file-changes" babysitKind="claude-recovery" /> },
      "babysit-claude-signed-out": { label: "Claude Code 未登录", render: () => <ReviewReaderPreview initialView="file-changes" babysitKind="claude-signed-out" /> },
      "babysit-custom-recovery": { label: "Custom 未配置恢复", render: () => <ReviewReaderPreview initialView="file-changes" babysitKind="custom-recovery" /> },
      "update-checking": { label: "正在检查更新", render: () => <ReviewReaderPreview updateState="checking" /> },
      "update-current": { label: "已是最新", render: () => <ReviewReaderPreview updateState="current" /> },
      "update-failed": { label: "检查失败", render: () => <ReviewReaderPreview updateState="failed" /> },
      update: { label: "检测到更新", render: () => <ReviewReaderPreview updateState="available" /> },
      merged: { label: "已合并", render: () => <ReviewReaderPreview merged /> },
      empty: { label: "无变更", render: () => <ReviewReaderPreview empty /> },
    },
  },
};

/** 将当前查询参数替换为指定的组件、场景与主题。 */
function navigatePreview(component: string, scenario: string, theme: string) {
  const query = new URLSearchParams({ component, scenario, theme });
  window.location.search = query.toString();
}

/** 渲染通用预览工具栏与当前选中的组件场景。 */
function PreviewApp() {
  const query = new URLSearchParams(window.location.search);
  const componentKey = query.get("component") ?? "branch-selector";
  const definition = previews[componentKey] ?? previews["branch-selector"];
  const requestedScenario = query.get("scenario") ?? "default";
  const scenarioKey = resolvePreviewScenarioKey(definition.scenarios, requestedScenario);
  const theme = query.get("theme") === "light" ? "light" : "dark";

  if (componentKey === "app-shell") return definition.scenarios[scenarioKey].render();

  return <main className="min-h-screen bg-background text-foreground">
    <header className="sticky top-0 z-50 flex flex-wrap items-end gap-4 border-b bg-background/90 px-5 py-3 backdrop-blur">
      <div className="mr-auto">
        <h1 className="font-heading text-base font-semibold">Narrant UI Preview</h1>
        <p className="text-xs text-muted-foreground">仅使用代表性数据，不连接 Tauri 状态</p>
      </div>
      <label className="grid gap-1 text-xs text-muted-foreground">
        组件
        <select
          className="h-9 rounded-lg border bg-background px-3 text-sm text-foreground"
          value={componentKey}
          onChange={(event) => navigatePreview(event.target.value, "default", theme)}
        >
          {Object.entries(previews).map(([key, preview]) => <option key={key} value={key}>{preview.label}</option>)}
        </select>
      </label>
      <label className="grid gap-1 text-xs text-muted-foreground">
        场景
        <select
          className="h-9 rounded-lg border bg-background px-3 text-sm text-foreground"
          value={scenarioKey}
          onChange={(event) => navigatePreview(componentKey, event.target.value, theme)}
        >
          {Object.entries(definition.scenarios).map(([key, scenario]) => <option key={key} value={key}>{scenario.label}</option>)}
        </select>
      </label>
      <label className="grid gap-1 text-xs text-muted-foreground">
        主题
        <select
          className="h-9 rounded-lg border bg-background px-3 text-sm text-foreground"
          value={theme}
          onChange={(event) => navigatePreview(componentKey, scenarioKey, event.target.value)}
        >
          <option value="dark">深色</option>
          <option value="light">浅色</option>
        </select>
      </label>
    </header>
    <section aria-label="组件预览画布" className="p-5 sm:p-8">
      {definition.scenarios[scenarioKey].render()}
    </section>
  </main>;
}

const initialTheme = new URLSearchParams(window.location.search).get("theme") === "light" ? "light" : "dark";
document.documentElement.classList.toggle("dark", initialTheme === "dark");
await i18n.changeLanguage("zh-CN");
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TooltipProvider><PreviewApp /></TooltipProvider>
  </StrictMode>,
);

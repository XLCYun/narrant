import i18n from "i18next";
import { initReactI18next } from "react-i18next";

/** 当前随应用离线打包的界面语言白名单。 */
export type SupportedLocale = "zh-CN" | "en";
export const localePreferenceKey = "narrant.locale";

/** 将任意 BCP-47 locale 收敛到离线资源白名单。 */
export function normalizeLocale(locale?: string): SupportedLocale {
  return locale?.toLowerCase().startsWith("en") ? "en" : "zh-CN";
}

/** 按显式偏好、系统语言、中文回退的顺序确定界面语言。 */
export function resolveLocale(explicit?: string, system?: string): SupportedLocale {
  return normalizeLocale(explicit || system);
}

/** 使用界面 locale 格式化绝对时间；测试可固定时区。 */
export function formatAbsoluteTime(
  value: number | Date,
  locale: SupportedLocale,
  timeZone?: string,
) {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
    timeZone,
  }).format(value);
}

/** 使用最接近的自然单位格式化相对时间。 */
export function formatRelativeTime(
  value: number,
  now: number,
  locale: SupportedLocale,
) {
  const elapsedSeconds = (value - now) / 1000;
  const units = [
    ["day", 86_400],
    ["hour", 3_600],
    ["minute", 60],
    ["second", 1],
  ] as const;
  const [unit, seconds] = units.find(([, size]) => Math.abs(elapsedSeconds) >= size) ?? units[3];
  return new Intl.RelativeTimeFormat(locale, { numeric: "auto" })
    .format(Math.round(elapsedSeconds / seconds), unit);
}

export const resources = {
  "zh-CN": {
    common: {
      brand: "纳言", subtitle: "好工程，纳良言", recent: "最近审核", emptyTitle: "纳言",
      emptyDescription: "好工程，纳良言", openFolder: "打开仓库文件夹",
      sidebar: "主侧边栏", collapse: "收起侧边栏", expand: "展开侧边栏", noRecent: "还没有审核记录",
      repositories: "代码仓库", viewAll: "查看全部", showLess: "收起", localBranch: "本地分支", openRepository: "打开仓库 {{name}}", openingRepository: "正在打开仓库 {{name}}",
      mergeDirection: "合入方向", lastOpened: "最近打开",
      loadingRecent: "正在加载最近审核", openingReview: "正在打开 Review", resizeSidebar: "调整侧边栏宽度", sidebarDescription: "显示移动端主侧边栏。", cancel: "取消", retry: "重试",
      trustTitle: "信任这个仓库？", trustDescription: "Narrant 需要执行 Git checkout。仓库配置可能运行 hooks、filters 和 Git LFS，这些能力可能执行仓库控制的程序或读取外部内容。",
      trustAndOpen: "信任并打开", repositoryNotGit: "所选文件夹不是 Git 仓库。", repositoryUnavailable: "所选文件夹不存在或无法访问。",
      repositoryRegistrationFailed: "无法打开所选仓库。", startReview: "开始审核", continueReview: "继续审核",
      sameBranchSelection: "Head 与 Base 不能是同一分支。", transientError: "操作失败",
    },
    repository: {
      sidebar: "仓库侧边栏", tasks: "仓库任务", tabs: { pullRequests: "Pull Requests", branches: "本地分支", history: "审核历史" },
      refreshing: "正在后台刷新 Pull Requests。", stale: "在线元数据可能不是最新。", updateAvailable: "有更新",
      draft: "草稿", noPullRequests: "没有可审核的 Pull Request", noPullRequestsDescription: "当前 GitHub 仓库暂时没有可审核的 Pull Request。",
      connectForPrivate: "连接 GitHub 以访问授权范围内的 Pull Request",
      loadFailed: "无法加载 Pull Requests", partialFailure: "{{repository}} 暂时无法加载：{{error}}",
      retry: "重新尝试", noHistory: "尚无审核历史",
      back: "返回仓库列表", localRepository: "选择本地仓库", repositoryPath: "本地仓库路径",
      reloadBranches: "重新读取分支", reloadingBranches: "正在重新读取分支", selectBase: "选择 Base 分支", selectHead: "选择 Head 分支",
      branchSelectionTitle: "选择要审核的分支", headBranch: "Head 分支", baseBranch: "Base 分支",
      currentBranch: "当前分支", defaultBranch: "默认分支", localBranchesGroup: "本地分支",
      remoteTrackingBranchesGroup: "远程跟踪分支", mergeConfirmation: "将 {{head}} 合入 {{base}}",
      searchBranches: "搜索分支…", noMatchingBranches: "没有匹配的分支",
      chooseBothBranches: "请选择 Head 与 Base 分支",
      checkingReviewUpdate: "正在检查审核更新", checkingReviewUpdateMessage: "正在检查审核内容是否有更新…",
      branchUpdateAvailable: "检测到所选分支有新提交", branchUpdateDescription: "将使用最新的分支内容继续审核。",
      updateAndContinueReview: "更新并继续审核",
      remoteUpdateCheckFailed: "无法检查 {{branch}} 的远程更新", branchUpdateCheckFailed: "无法检查分支更新", reviewMayBeStale: "当前审核版本可能不是最新",
      continueExistingReview: "继续现有审核",
    },
    review: {
      reader: "审核阅读器", diffLayout: "Diff 布局", inline: "单栏", sideBySide: "双栏", emptyTitle: "没有文件改动",
      emptyDescription: "Base 与 Head 比较成功，但内容没有差异。", fileChanges: "文件改动", changeCount: "{{count}} 个变更",
      fileTree: "文件树", resizeFileTree: "调整文件树宽度", fileChange: "File Change", loadingFileChange: "正在读取 File Change",
      binary: "二进制文件（变更前：{{before}}；变更后：{{after}}）", none: "无", bytes: "{{count}} 字节",
      submodule: "Submodule commit 变更", symlink: "Symlink 目标变更", added: "新增", modified: "修改", deleted: "删除", renamed: "重命名",
      openOverview: "打开审核概览", closeOverview: "关闭审核概览", overview: "审核概览", reviewViews: "审核视图", toolbar: "审核工具栏",
      pullRequestOverviewDescription: "Pull Request 的协作背景与完整 revisions", branchOverviewDescription: "本地分支比较与完整 revisions",
      localBranchReview: "本地分支审核", pullRequest: "Pull Request", author: "作者", status: "状态", draft: "草稿",
      state: { open: "已开启", closed: "已关闭", merged: "已合并" }, revisions: "Revisions", description: "描述",
      blockedRemoteImage: "远程图片已阻止加载", blockedRemoteImageWithAlt: "远程图片“{{alt}}”已阻止加载", viewOnGitHub: "在 GitHub 查看",
      branchCopied: "分支名已复制", revisionCopied: "Commit SHA 已复制", referenceCopyFailed: "引用复制失败", loadRemoteImage: "在浏览器中打开远程图片",
      mergeSentenceStart: "将", mergeSentenceJoin: "合入", revisionRelationLabel: "将 Head 分支 {{head}} 合入 Base 分支 {{base}}",
      moreActions: "更多操作", close: "关闭审核", exit: "退出审核", refresh: "刷新审核", refreshing: "正在刷新审核…", latest: "当前已是最新版本",
      currentAvailable: "当前 {{current}} · 可用 {{available}}", pullRequestBranchesUpdated: "检测到 PR 分支有更新", selectedBranchesUpdated: "检测到所选分支有更新", pullRequestUpdate: "Pull Request 更新", branchUpdate: "Branch Review 更新",
      switchToInline: "切换为单栏", switchToSideBySide: "切换为双栏", narrowInlineOnly: "当前窗口宽度仅支持单栏",
      babysit: "宝宝模式", babysitDisable: "关闭宝宝模式", babysitCancel: "取消宝宝模式", babysitExplanations: "逐行解释", babysitBefore: "变更前", babysitAfter: "变更后",
      babysitFilter: "逐行解释显示范围", babysitChanged: "仅改变行", babysitAll: "全部非空行", babysitExpand: "展开", babysitCollapse: "收起",
      babysitGenerating: "正在生成逐行解释…", babysitRepairing: "正在修复逐行解释（第 {{attempt}} 次）…", babysitFailed: "逐行解释失败",
      babysitCodexMissing: "未找到可用的 Codex，请前往应用设置的宝宝模式分区配置", babysitCodexSignedOut: "Codex 尚未登录，请运行 codex login 后重试", babysitCodexIncompatible: "当前 Codex 缺少宝宝模式所需能力，请升级后重试", babysitCodexInitializationFailed: "Codex 初始化失败，请重试",
      babysitClaudeMissing: "未找到可用的 Claude Code，请前往应用设置的宝宝模式分区配置", babysitClaudeSignedOut: "Claude Code 尚未登录，请先在终端完成登录", babysitClaudeIncompatible: "当前 Claude Code 缺少宝宝模式所需能力，请升级后重试", babysitClaudeInitializationFailed: "Claude Code 初始化失败，请重试",
      babysitCopyDiagnostics: "复制诊断", babysitCopiedDiagnostics: "已复制诊断", babysitOpenApplicationSettings: "打开应用设置", babysitRegenerate: "重新生成逐行解释",
      babysitMissingContent: "当前 File Change 尚未加载完成", babysitUnsupportedBinary: "不支持二进制文件", babysitUnsupportedSubmodule: "不支持 Submodule 变更", babysitUnsupportedSymlink: "不支持 Symlink 变更",
      babysitUnsupportedGenerated: "不分析文档或生成文件", babysitUnsupportedDependency: "不分析依赖、构建产物或 vendored 文件", babysitUnsupportedLock: "不分析 lock 文件", babysitUnsupportedMinified: "不分析压缩文件", babysitUnsupportedSnapshot: "不分析 snapshot 文件", babysitUnsupportedExtension: "文件扩展名不在代码语言白名单中",
      fileChangeError: "无法读取 File Change", closeDialog: "关闭",
    },
    github: {
      connect: "连接 GitHub", connected: "GitHub 已连接", signOut: "退出 GitHub",
      retryStatus: "重试 GitHub 状态", retryAction: "重试 GitHub 操作",
      unconfigured: "此构建未配置 GitHub App；公开 Pull Request 仍可匿名发现。",
      installHelp: "请在打开的网页中安装 Narrant GitHub App，并为目标仓库授予读取权限（组织仓库可能需要管理员审批）",
      installedContinue: "我已安装，继续", deviceCode: "设备码",
      copyDeviceCode: "复制设备码", deviceCodeCopied: "设备码已复制", copyLink: "复制链接", linkCopied: "链接已复制",
      authorizationPromptBefore: "请在打开的网页", authorizationPromptAfter: "中输入设备码以授权此设备访问你的 GitHub 账号",
      open: "打开 GitHub", safety: "请核对应用名称，不要输入他人发送的验证码。", authorizationComplete: "我已在 GitHub 完成授权",
    },
  },
  en: {
    common: {
      brand: "Narrant", subtitle: "Read-only code review", recent: "Recent reviews", emptyTitle: "Narrant",
      emptyDescription: "Read-only code review", openFolder: "Open repository folder",
      sidebar: "Primary sidebar", collapse: "Collapse sidebar", expand: "Expand sidebar", noRecent: "No reviews yet",
      repositories: "Repositories", viewAll: "View all", showLess: "Show less", localBranch: "Local branch", openRepository: "Open repository {{name}}", openingRepository: "Opening repository {{name}}",
      mergeDirection: "Merge direction", lastOpened: "Last opened",
      loadingRecent: "Loading recent reviews", openingReview: "Opening Review", resizeSidebar: "Resize sidebar", sidebarDescription: "Displays the primary sidebar on small windows.", cancel: "Cancel", retry: "Retry",
      trustTitle: "Trust this repository?", trustDescription: "Narrant needs to perform Git checkout. Repository configuration may run hooks, filters, and Git LFS, which can execute repository-controlled programs or read external content.",
      trustAndOpen: "Trust and open", repositoryNotGit: "The selected folder is not a Git repository.", repositoryUnavailable: "The selected folder does not exist or cannot be accessed.",
      repositoryRegistrationFailed: "The selected repository could not be opened.", startReview: "Start review", continueReview: "Continue review",
      sameBranchSelection: "Head and Base must be different branches.", transientError: "Operation failed",
    },
    repository: {
      sidebar: "Repository sidebar", tasks: "Repository tasks", tabs: { pullRequests: "Pull Requests", branches: "Local branches", history: "Review history" },
      refreshing: "Refreshing Pull Requests in the background.", stale: "Online metadata may be out of date.", updateAvailable: "Update available",
      draft: "Draft", noPullRequests: "No Pull Requests to review", noPullRequestsDescription: "The current GitHub repositories have no Pull Requests to review.",
      connectForPrivate: "Connect GitHub to access authorized Pull Requests",
      loadFailed: "Could not load Pull Requests", partialFailure: "{{repository}} could not be loaded: {{error}}",
      retry: "Try again", noHistory: "No review history",
      back: "Back to repositories", localRepository: "Choose local repository", repositoryPath: "Local repository path",
      reloadBranches: "Reload branches", reloadingBranches: "Reloading branches", selectBase: "Select Base branch", selectHead: "Select Head branch",
      branchSelectionTitle: "Choose branches to review", headBranch: "Head branch", baseBranch: "Base branch",
      currentBranch: "Current branch", defaultBranch: "Default branch", localBranchesGroup: "Local branches",
      remoteTrackingBranchesGroup: "Remote-tracking branches", mergeConfirmation: "Merge {{head}} into {{base}}",
      searchBranches: "Search branches…", noMatchingBranches: "No matching branches",
      chooseBothBranches: "Choose Head and Base branches",
      checkingReviewUpdate: "Checking for review updates", checkingReviewUpdateMessage: "Checking whether the review content has updates…",
      branchUpdateAvailable: "The selected branches have new commits", branchUpdateDescription: "Continue the review with the latest branch content.",
      updateAndContinueReview: "Update and continue review",
      remoteUpdateCheckFailed: "Could not check remote updates for {{branch}}", branchUpdateCheckFailed: "Could not check branch updates", reviewMayBeStale: "The current review version may not be up to date",
      continueExistingReview: "Continue existing review",
    },
    review: {
      reader: "Review reader", diffLayout: "Diff layout", inline: "Inline", sideBySide: "Side by side", emptyTitle: "No file changes",
      emptyDescription: "Base and Head were compared successfully, but their contents do not differ.", fileChanges: "File Changes", changeCount: "{{count}} changes",
      fileTree: "File tree", resizeFileTree: "Resize file tree width", fileChange: "File Change", loadingFileChange: "Loading File Change",
      binary: "Binary file (before: {{before}}; after: {{after}})", none: "none", bytes: "{{count}} bytes",
      submodule: "Submodule commit changed", symlink: "Symlink target changed", added: "Added", modified: "Modified", deleted: "Deleted", renamed: "Renamed",
      openOverview: "Open review overview", closeOverview: "Close review overview", overview: "Review overview", reviewViews: "Review views", toolbar: "Review toolbar",
      pullRequestOverviewDescription: "Pull Request context and full revisions", branchOverviewDescription: "Local branch comparison and full revisions",
      localBranchReview: "Local branch review", pullRequest: "Pull Request", author: "Author", status: "Status", draft: "Draft",
      state: { open: "Open", closed: "Closed", merged: "Merged" }, revisions: "Revisions", description: "Description",
      blockedRemoteImage: "Remote image was blocked", blockedRemoteImageWithAlt: "Remote image \"{{alt}}\" was blocked", viewOnGitHub: "View on GitHub",
      branchCopied: "Branch name copied", revisionCopied: "Commit SHA copied", referenceCopyFailed: "Could not copy reference", loadRemoteImage: "Open remote image in browser",
      mergeSentenceStart: "Merge", mergeSentenceJoin: "into", revisionRelationLabel: "Merge Head branch {{head}} into Base branch {{base}}",
      moreActions: "More actions", close: "Close review", exit: "Exit review", refresh: "Refresh review", refreshing: "Refreshing review…", latest: "Already up to date",
      currentAvailable: "Current {{current}} · available {{available}}", pullRequestBranchesUpdated: "PR branches have updates", selectedBranchesUpdated: "Selected branches have updates", pullRequestUpdate: "Pull Request update", branchUpdate: "Branch review update",
      switchToInline: "Switch to inline", switchToSideBySide: "Switch to side by side", narrowInlineOnly: "Only inline layout is available at this window width",
      babysit: "Babysit", babysitDisable: "Close Babysit", babysitCancel: "Cancel Babysit", babysitExplanations: "Line explanations", babysitBefore: "Before", babysitAfter: "After",
      babysitFilter: "Line explanation visibility", babysitChanged: "Changed lines", babysitAll: "All non-empty lines", babysitExpand: "Expand", babysitCollapse: "Collapse",
      babysitGenerating: "Generating line explanations…", babysitRepairing: "Repairing line explanations (attempt {{attempt}})…", babysitFailed: "Line explanations failed",
      babysitCodexMissing: "No usable Codex found; configure it in Application Settings under Babysit", babysitCodexSignedOut: "Codex is not signed in; run codex login and retry", babysitCodexIncompatible: "This Codex lacks Babysit capabilities; upgrade and retry", babysitCodexInitializationFailed: "Codex initialization failed; retry",
      babysitClaudeMissing: "No usable Claude Code found; configure it in Application Settings under Babysit", babysitClaudeSignedOut: "Claude Code is not signed in; sign in from a terminal", babysitClaudeIncompatible: "This Claude Code lacks Babysit capabilities; upgrade and retry", babysitClaudeInitializationFailed: "Claude Code initialization failed; retry",
      babysitCopyDiagnostics: "Copy diagnostics", babysitCopiedDiagnostics: "Diagnostics copied", babysitOpenApplicationSettings: "Open Application Settings", babysitRegenerate: "Regenerate line explanations",
      babysitMissingContent: "The File Change has not finished loading", babysitUnsupportedBinary: "Binary files are not supported", babysitUnsupportedSubmodule: "Submodule changes are not supported", babysitUnsupportedSymlink: "Symlink changes are not supported",
      babysitUnsupportedGenerated: "Documentation and generated files are not analyzed", babysitUnsupportedDependency: "Dependencies, build output, and vendored files are not analyzed", babysitUnsupportedLock: "Lock files are not analyzed", babysitUnsupportedMinified: "Minified files are not analyzed", babysitUnsupportedSnapshot: "Snapshot files are not analyzed", babysitUnsupportedExtension: "The file extension is outside the code-language allowlist",
      fileChangeError: "Could not load File Change", closeDialog: "Close",
    },
    github: {
      connect: "Connect GitHub", connected: "GitHub connected", signOut: "Sign out of GitHub",
      retryStatus: "Retry GitHub status", retryAction: "Retry GitHub action",
      unconfigured: "This build has no GitHub App configured; public Pull Requests remain available anonymously.",
      installHelp: "Install the Narrant GitHub App with repository read access in the opened webpage (organization repositories may require administrator approval).",
      installedContinue: "Installed, continue", deviceCode: "Device code",
      copyDeviceCode: "Copy device code", deviceCodeCopied: "Device code copied", copyLink: "Copy link", linkCopied: "Link copied",
      authorizationPromptBefore: "On the opened webpage", authorizationPromptAfter: "enter the device code to authorize this device to access your GitHub account.",
      open: "Open GitHub", safety: "Verify the app name and never enter a code sent by someone else.", authorizationComplete: "I completed authorization on GitHub",
    },
  },
} as const;

void i18n.use(initReactI18next).init({
  resources,
  lng: resolveLocale(window.localStorage?.getItem(localePreferenceKey) ?? undefined, navigator.language),
  fallbackLng: "zh-CN",
  defaultNS: "common",
  ns: ["common", "repository", "review", "github"],
  interpolation: { escapeValue: false },
});

if (import.meta.env.VITE_WDIO === "true") {
  /** 让真实 Tauri E2E 显式固定界面 locale，避免依赖 CI 主机语言。 */
  (window as Window & { __NARRANT_E2E_SET_LOCALE__?: (locale: string) => Promise<unknown> })
    .__NARRANT_E2E_SET_LOCALE__ = (locale) => i18n.changeLanguage(normalizeLocale(locale));
}

export { i18n };

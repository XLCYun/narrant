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
      brand: "纳言", subtitle: "审好代码，睡安稳觉", recent: "最近审核", emptyTitle: "纳言：审好代码，睡安稳觉",
      emptyDescription: "从本地仓库开始，只读地审好每一次代码变更。", openFolder: "打开仓库文件夹",
      sidebar: "主侧边栏", collapse: "收起侧边栏", expand: "展开侧边栏", noRecent: "还没有审核记录",
      repositories: "代码仓库", viewAll: "查看全部", showLess: "收起", localBranch: "本地分支", openRepository: "打开仓库 {{name}}", openingRepository: "正在打开仓库 {{name}}",
      mergeDirection: "合入方向", lastOpened: "最近打开",
      loadingRecent: "正在加载最近审核", resizeSidebar: "调整侧边栏宽度", sidebarDescription: "显示移动端主侧边栏。", cancel: "取消", retry: "重试",
      trustTitle: "信任这个仓库？", trustDescription: "Narrant 需要执行 Git checkout。仓库配置可能运行 hooks、filters 和 Git LFS，这些能力可能执行仓库控制的程序或读取外部内容。",
      trustAndOpen: "信任并打开", repositoryNotGit: "所选文件夹不是 Git 仓库。", repositoryUnavailable: "所选文件夹不存在或无法访问。",
      repositoryRegistrationFailed: "无法打开所选仓库。", startReview: "开始审核", continueReview: "继续审核",
      sameBranchSelection: "Base 与 Head 不能是同一个分支。", transientError: "操作失败",
    },
    catalog: {
      sidebar: "仓库侧边栏", tasks: "仓库任务", tabs: { pullRequests: "Pull Requests", branches: "本地分支", history: "审核历史" },
      refreshing: "正在后台刷新 Pull Requests。", stale: "在线元数据可能不是最新。", updateAvailable: "有更新",
      draft: "草稿", noPullRequests: "没有可审核的 Pull Request", noPullRequestsDescription: "当前 GitHub 仓库暂时没有可审核的 Pull Request。",
      connectForPrivate: "连接 GitHub 以访问授权范围内的 Pull Request", connectForPrivateDescription: "连接后可以发现当前账号有权访问的私有仓库 Pull Request。",
      loadFailed: "无法加载 Pull Requests", partialFailure: "{{repository}} 暂时无法加载：{{error}}",
      retry: "重新尝试", noHistory: "尚无审核历史",
      back: "返回仓库列表", localRepository: "选择本地仓库", repositoryPath: "本地仓库路径",
      reloadBranches: "重新读取分支", selectBase: "选择 Base Ref", selectHead: "选择 Head Ref",
    },
    review: {
      reader: "审核阅读器", diffLayout: "Diff 布局", inline: "单栏", sideBySide: "双栏", emptyTitle: "没有 File Change",
      emptyDescription: "Base 与 Head 比较成功，但内容没有差异。", fileChanges: "File Changes", changeCount: "{{count}} 个变更",
      fileTree: "文件树", resizeFileTree: "调整文件树宽度", fileChange: "File Change", loadingFileChange: "正在读取 File Change",
      binary: "二进制文件（变更前：{{before}}；变更后：{{after}}）", none: "无", bytes: "{{count}} 字节",
      submodule: "Submodule commit 变更", symlink: "Symlink 目标变更", added: "新增", modified: "修改", deleted: "删除", renamed: "重命名",
      openOverview: "打开审核概览", closeOverview: "关闭审核概览", overview: "审核概览",
      pullRequestOverviewDescription: "Pull Request 的协作背景与完整 revisions", branchOverviewDescription: "本地分支比较与完整 revisions",
      localBranchReview: "本地分支审核", pullRequest: "Pull Request", author: "作者", status: "状态", draft: "草稿",
      state: { open: "开放", closed: "已关闭", merged: "已合并" }, revisions: "Revisions", description: "描述",
      blockedRemoteImage: "远程图片已阻止加载", blockedRemoteImageWithAlt: "远程图片“{{alt}}”已阻止加载", viewOnGitHub: "在 GitHub 查看",
      moreActions: "更多操作", close: "关闭审核", refresh: "刷新审核", refreshing: "正在刷新审核…", latest: "当前已是最新版本",
      currentAvailable: "当前 {{current}} · 可用 {{available}}", pullRequestUpdate: "Pull Request 更新", branchUpdate: "Branch Review 更新",
      fileChangeError: "无法读取 File Change", closeDialog: "关闭",
    },
    github: {
      connect: "连接 GitHub", connected: "GitHub 已连接", signOut: "退出 GitHub",
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
      brand: "Narrant", subtitle: "Read-only code review", recent: "Recent reviews", emptyTitle: "Narrant: review code, sleep soundly",
      emptyDescription: "Start from a local repository and review every change read-only.", openFolder: "Open repository folder",
      sidebar: "Primary sidebar", collapse: "Collapse sidebar", expand: "Expand sidebar", noRecent: "No reviews yet",
      repositories: "Repositories", viewAll: "View all", showLess: "Show less", localBranch: "Local branch", openRepository: "Open repository {{name}}", openingRepository: "Opening repository {{name}}",
      mergeDirection: "Merge direction", lastOpened: "Last opened",
      loadingRecent: "Loading recent reviews", resizeSidebar: "Resize sidebar", sidebarDescription: "Displays the primary sidebar on small windows.", cancel: "Cancel", retry: "Retry",
      trustTitle: "Trust this repository?", trustDescription: "Narrant needs to perform Git checkout. Repository configuration may run hooks, filters, and Git LFS, which can execute repository-controlled programs or read external content.",
      trustAndOpen: "Trust and open", repositoryNotGit: "The selected folder is not a Git repository.", repositoryUnavailable: "The selected folder does not exist or cannot be accessed.",
      repositoryRegistrationFailed: "The selected repository could not be opened.", startReview: "Start review", continueReview: "Continue review",
      sameBranchSelection: "Base and Head must be different branches.", transientError: "Operation failed",
    },
    catalog: {
      sidebar: "Repository sidebar", tasks: "Repository tasks", tabs: { pullRequests: "Pull Requests", branches: "Local branches", history: "Review history" },
      refreshing: "Refreshing Pull Requests in the background.", stale: "Online metadata may be out of date.", updateAvailable: "Update available",
      draft: "Draft", noPullRequests: "No Pull Requests to review", noPullRequestsDescription: "The current GitHub repositories have no Pull Requests to review.",
      connectForPrivate: "Connect GitHub to access authorized Pull Requests", connectForPrivateDescription: "Connect to discover private-repository Pull Requests available to your account.",
      loadFailed: "Could not load Pull Requests", partialFailure: "{{repository}} could not be loaded: {{error}}",
      retry: "Try again", noHistory: "No review history",
      back: "Back to repositories", localRepository: "Choose local repository", repositoryPath: "Local repository path",
      reloadBranches: "Reload branches", selectBase: "Select Base Ref", selectHead: "Select Head Ref",
    },
    review: {
      reader: "Review reader", diffLayout: "Diff layout", inline: "Inline", sideBySide: "Side by side", emptyTitle: "No file changes",
      emptyDescription: "Base and Head were compared successfully, but their contents do not differ.", fileChanges: "File Changes", changeCount: "{{count}} changes",
      fileTree: "File tree", resizeFileTree: "Resize file tree width", fileChange: "File Change", loadingFileChange: "Loading File Change",
      binary: "Binary file (before: {{before}}; after: {{after}})", none: "none", bytes: "{{count}} bytes",
      submodule: "Submodule commit changed", symlink: "Symlink target changed", added: "Added", modified: "Modified", deleted: "Deleted", renamed: "Renamed",
      openOverview: "Open review overview", closeOverview: "Close review overview", overview: "Review overview",
      pullRequestOverviewDescription: "Pull Request context and full revisions", branchOverviewDescription: "Local branch comparison and full revisions",
      localBranchReview: "Local branch review", pullRequest: "Pull Request", author: "Author", status: "Status", draft: "Draft",
      state: { open: "Open", closed: "Closed", merged: "Merged" }, revisions: "Revisions", description: "Description",
      blockedRemoteImage: "Remote image was blocked", blockedRemoteImageWithAlt: "Remote image \"{{alt}}\" was blocked", viewOnGitHub: "View on GitHub",
      moreActions: "More actions", close: "Close review", refresh: "Refresh review", refreshing: "Refreshing review…", latest: "Already up to date",
      currentAvailable: "Current {{current}} · available {{available}}", pullRequestUpdate: "Pull Request update", branchUpdate: "Branch review update",
      fileChangeError: "Could not load File Change", closeDialog: "Close",
    },
    github: {
      connect: "Connect GitHub", connected: "GitHub connected", signOut: "Sign out of GitHub",
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
  ns: ["common", "catalog", "review", "github"],
  interpolation: { escapeValue: false },
});

if (import.meta.env.VITE_WDIO === "true") {
  /** 让真实 Tauri E2E 显式固定界面 locale，避免依赖 CI 主机语言。 */
  (window as Window & { __NARRANT_E2E_SET_LOCALE__?: (locale: string) => Promise<unknown> })
    .__NARRANT_E2E_SET_LOCALE__ = (locale) => i18n.changeLanguage(normalizeLocale(locale));
}

export { i18n };

import i18n from "i18next";
import { initReactI18next } from "react-i18next";

/** 当前随应用离线打包的界面语言白名单。 */
export type SupportedLocale = "zh-CN" | "en";

let e2eLocaleOverride: SupportedLocale | undefined;

/** 将任意 BCP-47 locale 收敛到离线资源白名单。 */
export function normalizeLocale(locale?: string): SupportedLocale {
  return locale?.toLowerCase().startsWith("en") ? "en" : "zh-CN";
}

/** 按显式偏好、系统语言、中文回退的顺序确定界面语言。 */
export function resolveLocale(explicit?: string, system?: string): SupportedLocale {
  return normalizeLocale(explicit || system);
}

/** 同步应用语言；真实 Tauri E2E 的显式覆盖优先于异步加载的持久化设置。 */
export async function applyApplicationLocale(locale: SupportedLocale): Promise<void> {
  const effective = e2eLocaleOverride ?? locale;
  await i18n.changeLanguage(effective);
  document.documentElement.lang = effective === "en" ? "en-US" : "zh-CN";
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
    structure: {
      title: "结构改动", unavailable: "结构分析暂不可用", loading: "正在分析 Rust 结构", loadingDescription: "正在比较当前审核版本的代码对象与关系。首次分析可能需要较长时间。",
      cancelled: "已取消结构分析", failed: "结构分析失败", retryDescription: "当前审核内容仍可继续阅读。你可以重新尝试结构分析。",
      changedCount: "{{count}} 个变化对象", visibleCount: "显示 {{visible}} / {{total}} 个对象", coverage: "覆盖 {{count}} / {{total}} 个文件", partialCoverage: "部分覆盖 · {{count}} / {{total}} 个文件",
      showAll: "显示全部对象", showUnchangedReferences: "显示未变化关系", layoutLoading: "正在排列结构图", layoutFailed: "无法排列结构图", retryLayout: "重新排列",
      displayOptions: "显示选项", showImports: "显示导入关系", showDependencies: "显示仓库外依赖",
      repositoryTarget: "仓库内 · 图外", dependencyTarget: "仓库外依赖", noVisibleReferences: "当前筛选下没有关联目标",
      emptyTitle: "没有结构变化", emptyDescription: "已分析范围内的对象与关系没有差异。可显示全部对象查看上下文。", unsupportedTitle: "这些文件暂不支持结构分析", unsupportedDescription: "当前支持 Rust 源文件。请查看覆盖范围，继续使用文件改动阅读其他内容。",
      filteredTitle: "没有可显示的结构变化", filteredDescription: "当前变化涉及的对象未在结构图中展示。导入声明等源码变化可在文件改动中查看。",
      legend: "结构变化图例", status: { added: "新增", removed: "删除", modified: "自身修改", relations: "关系变化", unchanged: "未变化" },
      memberSections: { attributes: "属性", methods: "方法与实现", values: "数据声明", functions: "函数与实现" },
      relationLegend: "关系：+ 新增 · − 虚线删除", expandChildren: "展开 {{count}} 个对象", external: "已解析 · 图外", unresolved: "未解析",
      zoomIn: "放大结构图", zoomOut: "缩小结构图", fitView: "适应画布", detailsTitle: "对象详情", detailsDescription: "查看前后声明、自身实现内容及关系变化。",
      coverageTitle: "分析覆盖范围", coverageDescription: "分别列出当前审核版本中每个变更文件的分析情况。", coverageKind: { analyzed: "已分析", absent: "该侧不存在", "unsupported-kind": "不支持的文件类型", "unsupported-language": "暂不支持的语言" },
      before: "变更前", after: "变更后", absent: "该侧没有此对象", noSignature: "没有声明签名", ownCode: "自身实现", noCode: "没有自身实现内容", relationshipChanges: "关系变化", noRelationshipChanges: "当前筛选下没有关系变化", referencesTitle: "关联目标", languageDetails: "语言特有信息",
    },
    review: {
      reader: "审核阅读器", diffLayout: "Diff 布局", inline: "单栏", sideBySide: "双栏", emptyTitle: "没有文件改动",
      emptyDescription: "Base 与 Head 比较成功，但内容没有差异。", fileChanges: "文件改动", structureChanges: "结构改动", loadingStructureView: "正在加载结构视图", changeCount: "{{count}} 个变更",
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
      moreActions: "更多操作", close: "关闭审核", exit: "退出审核", checkUpdate: "检查更新", checkingUpdate: "正在检查更新", retryCheck: "重试检查", refresh: "刷新审核", retryRefresh: "重试刷新", recheck: "重新检查", refreshing: "正在刷新审核…", latest: "已是最新", updateAvailable: "有更新", refreshed: "Review 已刷新", checkFailed: "检查更新失败", refreshFailed: "刷新失败，当前版本未改变", confirmationStale: "上游版本已再次变化，请重新检查",
      currentAvailable: "当前 {{current}} · 可用 {{available}}", pullRequestBranchesUpdated: "检测到 PR 分支有更新", selectedBranchesUpdated: "检测到所选分支有更新", pullRequestUpdate: "Pull Request 更新", branchUpdate: "Branch Review 更新",
      switchToInline: "切换为单栏", switchToSideBySide: "切换为双栏", narrowInlineOnly: "当前窗口宽度仅支持单栏",
      babysit: "宝宝模式", babysitDisable: "关闭宝宝模式", babysitCancel: "取消宝宝模式", babysitExplanations: "逐行解释", babysitBefore: "变更前", babysitAfter: "变更后",
      babysitExpand: "展开", babysitCollapse: "收起",
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
    settings: {
      provider: { custom: "自定义" },
      allNonEmptyLines: { label: "生成全部非空行的注释", description: "关闭时仅解释变更的非空行；开启后解释全部非空行，生成时间和用量可能增加。" },
      repositoryExploration: {
        label: "允许仓库探索",
        description: "开启后，Agent 可以只读搜索当前 Review Workspace 以获取更多上下文，但生成可能明显变慢，并会向模型服务发送额外代码内容。",
      },
      custom: {
        title: "自定义模型服务",
        description: "填写一个 OpenAI Chat Completions 或 Anthropic Messages 兼容的模型服务连接。",
        protocol: "协议",
        openAiChatCompletions: "OpenAI Chat Completions",
        anthropicMessages: "Anthropic Messages",
        baseUrl: "Base URL",
        baseUrlPlaceholder: "https://example.com/v1",
        apiKey: "API 密钥",
        apiKeyPlaceholder: "可选；本地服务可以留空",
        apiKeyHint: "密钥仅用于当前连接，不会写入 settings.json。",
        model: "模型",
        modelPlaceholder: "输入模型标识",
        effort: "思考强度",
        providerDefault: "Provider 默认值",
        effortHint: "使用 Provider 默认值时，Narrant 不会发送推理控制参数；模型仍可能按自身默认策略进行推理。",
        low: "低",
        medium: "中",
        high: "高",
        save: "保存自定义连接",
        saving: "正在保存…",
        loading: "正在读取自定义连接…",
        loadFailed: "无法读取自定义连接，请检查设置后重试。",
        saveFailed: "无法保存自定义连接。",
        saveFailedPreserved: "已保留你刚才输入的内容，请修正后重试。",
        validationBaseUrl: "请填写 Base URL。",
        validationModel: "请填写模型。",
        showApiKey: "显示 API 密钥",
        hideApiKey: "隐藏 API 密钥",
      },
    },
    applicationUpdate: {
      settings: { title: "应用更新", description: "检查、下载并安全暂存 Narrant 更新。", currentVersion: "当前版本", availableVersion: "可用版本", status: "状态", automaticDownload: "自动下载更新", automaticDownloadDescription: "发现新版本后自动下载并验证；关闭后仍会定期检查。", loading: "正在读取更新状态…", loadFailed: "无法读取应用更新状态，请重试。" },
      state: { idle: "等待检查", checking: "正在检查", up_to_date: "已是最新版本", available: "有新版本可用", downloading: "正在下载更新", staged: "更新已准备就绪", failed: "更新失败" },
      attention: { available: "有应用更新可用", downloading: "正在下载应用更新", failed: "应用更新失败", staged: "应用更新已准备就绪" },
      action: { check: "检查更新", checking: "正在检查…", retry: "重试", retryLoad: "重新连接", details: "查看更新详情" },
      surface: { title: "应用更新", version: "Narrant {{version}}", releaseNotes: "版本说明", noNotes: "此版本没有提供版本说明。", later: "稍后", restart: "立即重启并更新", blockedImage: "远程图片已阻止加载", blockedImageWithAlt: "远程图片“{{alt}}”已阻止加载" },
      progress: { downloading: "正在下载", known: "{{percentage}}% · {{downloaded}} / {{total}}", determinateLabel: "更新已下载 {{percentage}}%", indeterminateLabel: "正在下载更新，文件总大小未知" },
      failure: { network: "无法连接应用更新服务，请检查网络后重试。", request_timeout: "应用更新请求超时，请稍后重试。", rate_limited: "应用更新检查过于频繁，请稍后重试。", server: "应用更新服务暂时不可用，请稍后重试。", manifest: "应用更新信息无效，已停止本次更新。", verification: "应用更新包验证失败，已安全丢弃。", storage: "应用更新无法安全写入本机存储。", installer: "应用更新安装失败，暂存包已保留。" },
      warning: { recovery: "上次暂存的更新无法恢复；你可以重新检查更新。", preference: { rejected: "自动下载偏好未能保存，已恢复原设置。", durability_unknown: "自动下载偏好已应用，但持久保存尚未确认。", state_unknown: "无法确认自动下载偏好的当前值，请稍后重试。" } },
      commandError: { "manual-check": "无法发起更新检查，请重试。", preference: "自动下载偏好未能保存，已恢复原设置。", retry: "无法重试更新，请稍后再试。", restart: "无法请求重启更新，请稍后再试。" },
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
    structure: {
      title: "Structure Changes", unavailable: "Structure analysis is unavailable", loading: "Analyzing Rust structure", loadingDescription: "Comparing code objects and relationships in this review. The first analysis may take a while.",
      cancelled: "Structure analysis cancelled", failed: "Structure analysis failed", retryDescription: "You can keep reading this review and retry structure analysis.",
      changedCount: "{{count}} changed objects", visibleCount: "Showing {{visible}} / {{total}} objects", coverage: "{{count}} / {{total}} files covered", partialCoverage: "Partial coverage · {{count}} / {{total}} files",
      showAll: "Show all objects", showUnchangedReferences: "Show unchanged relationships", layoutLoading: "Arranging structure graph", layoutFailed: "Unable to arrange the graph", retryLayout: "Retry layout",
      displayOptions: "Display options", showImports: "Show imports", showDependencies: "Show dependencies outside repository",
      repositoryTarget: "Repository · outside graph", dependencyTarget: "External dependency", noVisibleReferences: "No references match the current filters",
      emptyTitle: "No structural changes", emptyDescription: "Objects and relationships in the analyzed scope are unchanged. Show all objects to explore the context.", unsupportedTitle: "These files are not supported yet", unsupportedDescription: "Structure analysis currently supports Rust source files. Check coverage and use File Changes for other content.",
      filteredTitle: "No structural changes to display", filteredDescription: "The changed objects are not shown in this graph. See File Changes for import declarations and other source changes.",
      legend: "Structure change legend", status: { added: "Added", removed: "Removed", modified: "Content changed", relations: "Relationships changed", unchanged: "Unchanged" },
      memberSections: { attributes: "Attributes", methods: "Methods & implementations", values: "Data declarations", functions: "Functions & implementations" },
      relationLegend: "Relationships: + added · − dashed removed", expandChildren: "Expand {{count}} objects", external: "Resolved · outside graph", unresolved: "Unresolved",
      zoomIn: "Zoom in", zoomOut: "Zoom out", fitView: "Fit graph", detailsTitle: "Object details", detailsDescription: "Compare declarations, own implementation and relationship changes.",
      coverageTitle: "Analysis coverage", coverageDescription: "Analysis status for each side of the changed files in this review.", coverageKind: { analyzed: "Analyzed", absent: "Absent on this side", "unsupported-kind": "Unsupported file kind", "unsupported-language": "Unsupported language" },
      before: "Before", after: "After", absent: "Object absent on this side", noSignature: "No declaration signature", ownCode: "Own implementation", noCode: "No own implementation", relationshipChanges: "Relationship changes", noRelationshipChanges: "No relationship changes match the current filters", referencesTitle: "Reference targets", languageDetails: "Language-specific details",
    },
    review: {
      reader: "Review reader", diffLayout: "Diff layout", inline: "Inline", sideBySide: "Side by side", emptyTitle: "No file changes",
      emptyDescription: "Base and Head were compared successfully, but their contents do not differ.", fileChanges: "File Changes", structureChanges: "Structure Changes", loadingStructureView: "Loading structure view", changeCount: "{{count}} changes",
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
      moreActions: "More actions", close: "Close review", exit: "Exit review", checkUpdate: "Check for updates", checkingUpdate: "Checking for updates", retryCheck: "Retry check", refresh: "Refresh review", retryRefresh: "Retry refresh", recheck: "Check again", refreshing: "Refreshing review…", latest: "Up to date", updateAvailable: "Update available", refreshed: "Review refreshed", checkFailed: "Could not check for updates", refreshFailed: "Refresh failed; the current version is unchanged", confirmationStale: "Upstream changed again; check for updates again",
      currentAvailable: "Current {{current}} · available {{available}}", pullRequestBranchesUpdated: "PR branches have updates", selectedBranchesUpdated: "Selected branches have updates", pullRequestUpdate: "Pull Request update", branchUpdate: "Branch review update",
      switchToInline: "Switch to inline", switchToSideBySide: "Switch to side by side", narrowInlineOnly: "Only inline layout is available at this window width",
      babysit: "Babysit", babysitDisable: "Close Babysit", babysitCancel: "Cancel Babysit", babysitExplanations: "Line explanations", babysitBefore: "Before", babysitAfter: "After",
      babysitExpand: "Expand", babysitCollapse: "Collapse",
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
    settings: {
      provider: { custom: "Custom" },
      allNonEmptyLines: { label: "Explain all non-empty lines", description: "When disabled, explain only changed non-empty lines. Enabling this explains all non-empty lines and may increase generation time and usage." },
      repositoryExploration: {
        label: "Allow repository exploration",
        description: "When enabled, the Agent may search the current Review Workspace read-only for more context. Generation can take significantly longer and may send additional code to the model service.",
      },
      custom: {
        title: "Custom model service",
        description: "Configure an OpenAI Chat Completions or Anthropic Messages compatible model service.",
        protocol: "Protocol",
        openAiChatCompletions: "OpenAI Chat Completions",
        anthropicMessages: "Anthropic Messages",
        baseUrl: "Base URL",
        baseUrlPlaceholder: "https://example.com/v1",
        apiKey: "API key",
        apiKeyPlaceholder: "Optional; leave blank for local services",
        apiKeyHint: "The key is used only for this connection and is never written to settings.json.",
        model: "Model",
        modelPlaceholder: "Enter a model identifier",
        effort: "Reasoning effort",
        providerDefault: "Provider default",
        effortHint: "Provider default does not send reasoning controls from Narrant; the model may still reason using its own defaults.",
        low: "Low",
        medium: "Medium",
        high: "High",
        save: "Save custom connection",
        saving: "Saving…",
        loading: "Loading custom connection…",
        loadFailed: "Could not load the custom connection. Check settings and try again.",
        saveFailed: "Could not save the custom connection.",
        saveFailedPreserved: "Your entered values were kept. Correct them and try again.",
        validationBaseUrl: "Enter a Base URL.",
        validationModel: "Enter a model.",
        showApiKey: "Show API key",
        hideApiKey: "Hide API key",
      },
    },
    applicationUpdate: {
      settings: { title: "Application updates", description: "Check, download, and safely stage Narrant updates.", currentVersion: "Current version", availableVersion: "Available version", status: "Status", automaticDownload: "Download updates automatically", automaticDownloadDescription: "Download and verify new versions automatically. Narrant still checks for updates when this is off.", loading: "Loading update status…", loadFailed: "Could not load application update status. Try again." },
      state: { idle: "Waiting to check", checking: "Checking for updates", up_to_date: "Up to date", available: "New version available", downloading: "Downloading update", staged: "Update ready", failed: "Update failed" },
      attention: { available: "Application update available", downloading: "Downloading application update", failed: "Application update failed", staged: "Application update ready" },
      action: { check: "Check for updates", checking: "Checking…", retry: "Retry", retryLoad: "Reconnect", details: "View update details" },
      surface: { title: "Application update", version: "Narrant {{version}}", releaseNotes: "Release notes", noNotes: "No release notes were provided for this version.", later: "Later", restart: "Restart and Update", blockedImage: "Remote image blocked", blockedImageWithAlt: "Remote image “{{alt}}” blocked" },
      progress: { downloading: "Downloading", known: "{{percentage}}% · {{downloaded}} / {{total}}", determinateLabel: "Update {{percentage}}% downloaded", indeterminateLabel: "Downloading update; total size unknown" },
      failure: { network: "Could not connect to the update service. Check your network and retry.", request_timeout: "The update request timed out. Try again later.", rate_limited: "Update checks are being rate limited. Try again later.", server: "The update service is temporarily unavailable. Try again later.", manifest: "The update information is invalid. This update was stopped.", verification: "The update package could not be verified and was safely discarded.", storage: "The update could not be written safely to local storage.", installer: "Installation failed. The staged package was kept." },
      warning: { recovery: "The previously staged update could not be recovered. Check again to retry.", preference: { rejected: "The automatic download preference was not saved and has been restored.", durability_unknown: "The automatic download preference was applied, but durable storage is not confirmed.", state_unknown: "The current automatic download preference could not be confirmed. Try again later." } },
      commandError: { "manual-check": "Could not start the update check. Try again.", preference: "The automatic download preference was not saved and has been restored.", retry: "Could not retry the update. Try again later.", restart: "Could not request restart and update. Try again later." },
    },
  },
} as const;

void i18n.use(initReactI18next).init({
  resources,
  // Application Settings 加载前使用稳定的离线初始资源；启动后由设置快照唯一同步语言。
  lng: "zh-CN",
  fallbackLng: "zh-CN",
  defaultNS: "common",
  ns: ["common", "repository", "review", "structure", "github", "settings", "applicationUpdate"],
  interpolation: { escapeValue: false },
});
document.documentElement.lang = "zh-CN";

if (import.meta.env.VITE_WDIO === "true") {
  /** 让真实 Tauri E2E 显式固定界面 locale，避免依赖 CI 主机语言。 */
  (window as Window & { __NARRANT_E2E_SET_LOCALE__?: (locale: string) => Promise<unknown> })
    .__NARRANT_E2E_SET_LOCALE__ = async (locale) => {
      e2eLocaleOverride = normalizeLocale(locale);
      await applyApplicationLocale(e2eLocaleOverride);
    };
}

export { i18n };

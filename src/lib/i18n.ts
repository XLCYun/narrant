import i18n from "i18next";
import { initReactI18next } from "react-i18next";

/** 将系统 locale 收敛到当前离线资源支持的语言集合。 */
export function normalizeLocale(locale?: string): "zh-CN" | "en" {
  return locale?.toLowerCase().startsWith("en") ? "en" : "zh-CN";
}

const resources = {
  "zh-CN": {
    common: { brand: "纳言", subtitle: "只读代码审核", recent: "最近审核", emptyTitle: "纳言：审好代码，睡安稳觉", emptyDescription: "从本地仓库开始，只读地审好每一次代码变更。", openFolder: "打开仓库文件夹", sidebar: "主侧边栏", collapse: "收起侧边栏", expand: "展开侧边栏", noRecent: "还没有审核记录", trustTitle: "信任这个仓库？", trustDescription: "Narrant 需要执行 Git checkout。仓库配置可能运行 hooks、filters 和 Git LFS，这些能力可能执行仓库控制的程序或读取外部内容。", cancel: "取消", trustAndOpen: "信任并打开", repositoryNotGit: "所选文件夹不是 Git 仓库。", repositoryUnavailable: "所选文件夹不存在或无法访问。", repositoryRegistrationFailed: "无法打开所选仓库。" },
    review: { reader: "Review 阅读器", diffLayout: "Diff 布局", inline: "单栏", sideBySide: "双栏", emptyTitle: "没有 File Change", emptyDescription: "Base 与 Head 比较成功，但内容没有差异。", fileChanges: "File Changes", changeCount: "{{count}} 个变更", fileTree: "文件树", resizeFileTree: "调整文件树宽度", binary: "二进制文件（变更前：{{before}}；变更后：{{after}}）", none: "无", bytes: "{{count}} 字节", submodule: "Submodule commit 变更", symlink: "Symlink 目标变更", added: "新增", modified: "修改", deleted: "删除", renamed: "重命名" },
  },
  en: {
    common: { brand: "Narrant", subtitle: "Read-only code review", recent: "Recent reviews", emptyTitle: "Narrant: review code, sleep soundly", emptyDescription: "Start from a local repository and review every change read-only.", openFolder: "Open repository folder", sidebar: "Primary sidebar", collapse: "Collapse sidebar", expand: "Expand sidebar", noRecent: "No reviews yet", trustTitle: "Trust this repository?", trustDescription: "Narrant needs to perform Git checkout. Repository configuration may run hooks, filters, and Git LFS, which can execute repository-controlled programs or read external content.", cancel: "Cancel", trustAndOpen: "Trust and open", repositoryNotGit: "The selected folder is not a Git repository.", repositoryUnavailable: "The selected folder does not exist or cannot be accessed.", repositoryRegistrationFailed: "The selected repository could not be opened." },
    review: { reader: "Review reader", diffLayout: "Diff layout", inline: "Inline", sideBySide: "Side by side", emptyTitle: "No file changes", emptyDescription: "Base and Head were compared successfully, but their contents do not differ.", fileChanges: "File Changes", changeCount: "{{count}} changes", fileTree: "File tree", resizeFileTree: "Resize file tree", binary: "Binary file (before: {{before}}; after: {{after}})", none: "none", bytes: "{{count}} bytes", submodule: "Submodule commit changed", symlink: "Symlink target changed", added: "Added", modified: "Modified", deleted: "Deleted", renamed: "Renamed" },
  },
} as const;

void i18n.use(initReactI18next).init({
  resources,
  lng: normalizeLocale(navigator.language),
  fallbackLng: "zh-CN",
  defaultNS: "common",
  ns: ["common", "review"],
  interpolation: { escapeValue: false },
});

if (import.meta.env.VITE_WDIO === "true") {
  /** 让真实 Tauri E2E 显式固定界面 locale，避免依赖 CI 主机语言。 */
  (window as Window & { __NARRANT_E2E_SET_LOCALE__?: (locale: string) => Promise<unknown> })
    .__NARRANT_E2E_SET_LOCALE__ = (locale) => i18n.changeLanguage(normalizeLocale(locale));
}

export { i18n };

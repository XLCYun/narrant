import i18n from "i18next";
import { initReactI18next } from "react-i18next";

/** 将系统 locale 收敛到当前离线资源支持的语言集合。 */
export function normalizeLocale(locale?: string): "zh-CN" | "en" {
  return locale?.toLowerCase().startsWith("en") ? "en" : "zh-CN";
}

const resources = {
  "zh-CN": { common: { brand: "纳言", subtitle: "只读代码审核", recent: "最近审核", emptyTitle: "纳言：审好代码，睡安稳觉", emptyDescription: "从本地仓库开始，只读地审好每一次代码变更。", openFolder: "打开仓库文件夹", sidebar: "主侧边栏", collapse: "收起侧边栏", expand: "展开侧边栏", noRecent: "还没有审核记录" } },
  en: { common: { brand: "Narrant", subtitle: "Read-only code review", recent: "Recent reviews", emptyTitle: "Narrant: review code, sleep soundly", emptyDescription: "Start from a local repository and review every change read-only.", openFolder: "Open repository folder", sidebar: "Primary sidebar", collapse: "Collapse sidebar", expand: "Expand sidebar", noRecent: "No reviews yet" } },
} as const;

void i18n.use(initReactI18next).init({
  resources,
  lng: normalizeLocale(navigator.language),
  fallbackLng: "zh-CN",
  defaultNS: "common",
  ns: ["common"],
  interpolation: { escapeValue: false },
});

export { i18n };

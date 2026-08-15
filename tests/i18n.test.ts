import { afterEach, describe, expect, it } from "vitest";

import {
  formatAbsoluteTime,
  formatRelativeTime,
  i18n,
  normalizeLocale,
  resolveLocale,
} from "@/lib/i18n";

afterEach(async () => {
  window.localStorage.clear();
  await i18n.changeLanguage("zh-CN");
});

describe("离线国际化边界", () => {
  it("按显式偏好、系统 locale、中文回退的顺序选择语言", () => {
    expect(resolveLocale("en", "zh-CN")).toBe("en");
    expect(resolveLocale(undefined, "en-US")).toBe("en");
    expect(resolveLocale(undefined, "fr-FR")).toBe("zh-CN");
    expect(normalizeLocale("EN-gb")).toBe("en");
  });

  it("使用固定 locale 和时区格式化绝对与相对时间", () => {
    const now = Date.UTC(2026, 6, 29, 8, 0, 0);
    const earlier = now - 2 * 60 * 60 * 1000;

    expect(formatAbsoluteTime(now, "en", "UTC")).toContain("Jul");
    expect(formatAbsoluteTime(now, "zh-CN", "UTC")).toContain("2026");
    expect(formatRelativeTime(earlier, now, "en")).toBe("2 hours ago");
    expect(formatRelativeTime(earlier, now, "zh-CN")).toBe("2小时前");
  });

  it("离线提供 common、repository、review、github 四个 namespace", () => {
    expect(i18n.t("repository:tabs.branches", { lng: "en" })).toBe("Local branches");
    expect(i18n.t("github:connect", { lng: "en" })).toBe("Connect GitHub");
    expect(i18n.t("review:refresh", { lng: "zh-CN" })).toBe("刷新审核");
    expect(i18n.t("common:retry", { lng: "zh-CN" })).toBe("重试");
  });
});

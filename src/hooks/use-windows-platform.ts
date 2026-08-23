import { useMemo } from "react";

/** 浏览器平台探测所需的最小 Navigator 投影。 */
type NavigatorPlatformSource = Pick<Navigator, "platform" | "userAgent"> & {
  userAgentData?: { platform?: string };
};

/** 从浏览器暴露的平台信息判断当前是否运行在 Windows。 */
export function detectWindowsPlatform(source: NavigatorPlatformSource = navigator): boolean {
  const platform = source.userAgentData?.platform || source.platform || source.userAgent;
  return /windows|win32|win64/i.test(platform);
}

/** 提供稳定的 Windows 平台探测结果，供桌面布局按原生窗口装饰差异调整。 */
export function useWindowsPlatform(): boolean {
  return useMemo(() => detectWindowsPlatform(), []);
}

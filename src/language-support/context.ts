import { createContext } from "react";
import type { LanguageSupportBinding } from "./use-language-support-packs";

/** 页面共享应用层控制器，只读取同一轮询器的结果。 */
export const LanguageSupportContext = createContext<LanguageSupportBinding | undefined>(undefined);

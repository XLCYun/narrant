import { openFixtureRepository, setE2ELocale } from "./helpers/open-repository";
import { selectRef, visibleBrandButton } from "./helpers/review-navigation";

declare global {
  interface Window {
    __NARRANT_E2E_COPIED_DIAGNOSTIC__?: string;
  }
}

/** 读取显式 live probe 的连接字段；缺失时立即拒绝，避免误跑付费请求。 */
function liveValue(name: string) {
  if (process.env.NARRANT_E2E_LIVE_CUSTOM_PROVIDER !== "true") {
    throw new Error("live Custom Provider E2E 未显式启用");
  }
  const value = process.env[name];
  if (!value) throw new Error(`live Custom Provider E2E 缺少 ${name}`);
  return value;
}

/** 将冻结的 wire effort 映射到中文设置页的可见选项。 */
function effortLabel(effort: string) {
  const labels: Record<string, string> = {
    low: "低",
    medium: "中",
    high: "高",
  };
  const label = labels[effort];
  if (!label) throw new Error(`live Custom Provider E2E 不支持 effort：${effort}`);
  return label;
}

/** 点击响应式布局中当前可见且真正可交互的同名按钮。 */
async function clickVisibleButton(text: string) {
  await browser.waitUntil(async () => {
    for (const button of await $$(`button=${text}`)) {
      if (!(await button.isDisplayed()) || !(await button.isEnabled())) continue;
      const interactive = await browser.execute(
        (element) => getComputedStyle(element).pointerEvents !== "none",
        button,
      );
      if (!interactive) continue;
      await button.click();
      return true;
    }
    return false;
  }, { timeoutMsg: `没有找到可交互按钮：${text}` });
}

/** 通过真实设置 UI 把连接保存到本次隔离的 Tauri E2E 进程。 */
async function configureCustomProvider() {
  await setE2ELocale("zh-CN");
  await visibleBrandButton();
  const settings = await $("button[aria-label='设置']");
  await settings.waitForClickable();
  await settings.click();
  await expect($("[aria-label='应用设置']")).toBeDisplayed();
  await $("[role='tab']=宝宝模式").click();
  await expect($("[data-testid='custom-provider-editor']")).toBeDisplayed();

  const effortValue = liveValue("NARRANT_E2E_CUSTOM_PROVIDER_EFFORT");
  if (effortValue !== "provider_default") {
    const effort = effortLabel(effortValue);
    const effortSelect = await $("button[aria-label='思考强度']");
    await effortSelect.click();
    const option = await $(`[role='option']=${effort}`);
    await option.waitForClickable();
    await option.click();
    await browser.waitUntil(async () => await effortSelect.getAttribute("aria-expanded") === "false", {
      timeoutMsg: "思考强度选项没有关闭",
    });
  }
  await $("input[aria-label='Base URL']").setValue(liveValue("NARRANT_E2E_CUSTOM_PROVIDER_BASE_URL"));
  await $("input[aria-label='API 密钥']").setValue(process.env.NARRANT_E2E_CUSTOM_PROVIDER_API_KEY ?? "");
  await $("input[aria-label='模型']").setValue(liveValue("NARRANT_E2E_CUSTOM_PROVIDER_MODEL"));
  const save = await $("button=保存自定义连接");
  await save.waitForClickable();
  await save.click();
  await browser.waitUntil(async () => await save.isEnabled(), {
    timeoutMsg: "Custom Provider 连接没有完成保存",
  });
  await $("button[aria-label='返回']").click();
}

/** 从真实失败 UI 捕获已经过产品白名单脱敏的诊断。 */
async function captureStableDiagnostic() {
  await browser.execute(() => {
    window.__NARRANT_E2E_COPIED_DIAGNOSTIC__ = undefined;
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async (value: string) => {
          window.__NARRANT_E2E_COPIED_DIAGNOSTIC__ = value;
        },
      },
    });
  });
  const copy = await $("button[aria-label='复制诊断']");
  await copy.click();
  return browser.waitUntil(async () => browser.execute(
    () => window.__NARRANT_E2E_COPIED_DIAGNOSTIC__ || "",
  ), { timeoutMsg: "失败 UI 没有产生稳定诊断" });
}

/** 真实 Tauri WebView + 真实 endpoint 的显式付费 smoke；默认 WDIO specs 不包含此文件。 */
describe("Custom Provider live smoke", () => {
  it("通过设置 UI 保存连接并完成一次宝宝模式分析", async () => {
    await configureCustomProvider();
    const repositoryPath = process.env.NARRANT_E2E_REPOSITORY!;
    await openFixtureRepository(repositoryPath);
    await $("[role='tab']=本地分支").click();
    await selectRef("Base Ref", "refs/heads/base-ref");
    await selectRef("Head Ref", "refs/heads/head-ref");
    await clickVisibleButton("开始审核");
    await $("[aria-label='审核阅读器']").waitForDisplayed({
      timeout: 60_000,
      timeoutMsg: "本地 Review 没有在 60 秒内完成打开",
    });
    await expect($("[aria-label='File Change']")).toBeDisplayed();

    const codeFile = await $("button*=z-explain.ts");
    await codeFile.waitForClickable();
    await codeFile.click();
    const toggle = await $("button[aria-label='宝宝模式']");
    await browser.waitUntil(async () => await toggle.getAttribute("aria-disabled") === "false");
    await toggle.click();

    let outcome: "completed" | "failed" | "pending" = "pending";
    await browser.waitUntil(async () => {
      outcome = await browser.execute(() => {
        const footer = document.querySelector<HTMLElement>("[data-testid='babysit-footer']");
        if (footer?.querySelector("button[aria-label='复制诊断']")) return "failed" as const;
        const completed = [...document.querySelectorAll<HTMLButtonElement>("button")]
          .some((button) => button.textContent?.includes("全部非空行"));
        return completed ? "completed" as const : "pending" as const;
      });
      return outcome !== "pending";
    }, {
      timeout: 180_000,
      interval: 250,
      timeoutMsg: "Custom Provider 宝宝模式在 180 秒内既未完成也未返回稳定失败",
    });
    if (outcome !== "completed") {
      const diagnostic = await captureStableDiagnostic();
      throw new Error(`Custom Provider E2E failed: ${diagnostic}`);
    }
    const showAll = await $("button=全部非空行");
    await showAll.waitForClickable();
    await showAll.click();
    await expect($("button=仅改变行")).toBeDisplayed();
    await browser.waitUntil(async () => browser.execute(() => {
      const explanations = [...document.querySelectorAll<HTMLElement>(".babysit-inline-explanation")];
      return explanations.length > 0
        && explanations.every((explanation) => Boolean(explanation.textContent?.trim()));
    }), {
      timeout: 15_000,
      timeoutMsg: "Custom Provider 已完成，但 Diff 中没有渲染非空逐行解释",
    });
  });
});

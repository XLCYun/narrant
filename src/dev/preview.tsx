import { StrictMode, type ReactNode, useState } from "react";
import { createRoot } from "react-dom/client";

import "@/index.css";
import { BranchReviewSelector, type BranchSelectorAction } from "@/components/branch-review-selector";
import { TooltipProvider } from "@/components/ui/tooltip";
import { i18n } from "@/lib/i18n";

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
}: {
  refs?: string[];
  initialHead?: string;
  initialBase?: string;
  currentRef?: string;
  action?: BranchSelectorAction;
}) {
  const [headRef, setHeadRef] = useState(initialHead);
  const [baseRef, setBaseRef] = useState(initialBase);

  return <BranchReviewSelector
    refs={refs}
    currentRef={currentRef}
    defaultRef="refs/remotes/origin/main"
    headRef={headRef}
    baseRef={baseRef}
    action={action}
    onHeadChange={setHeadRef}
    onBaseChange={setBaseRef}
    onReload={() => undefined}
    onSubmit={() => undefined}
  />;
}

const previews: Record<string, PreviewDefinition> = {
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
        render: () => <BranchSelectorPreview action="update" />,
      },
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
  const scenarioKey = definition.scenarios[requestedScenario] ? requestedScenario : "default";
  const theme = query.get("theme") === "light" ? "light" : "dark";

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

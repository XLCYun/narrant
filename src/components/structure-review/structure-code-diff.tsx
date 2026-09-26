import { useCallback, useRef } from "react";
import { DiffEditor, type DiffOnMount } from "@monaco-editor/react";
import { useTranslation } from "react-i18next";
import { configureLocalMonaco } from "@/infrastructure/local-monaco";
import { registerReviewEditorThemes, REVIEW_DARK_THEME, REVIEW_LIGHT_THEME } from "@/review/editor-theme";

configureLocalMonaco();

/** 填满侧栏剩余高度并在编辑器内滚动；长行仅视觉折行，不格式化或改写源码。 */
export function StructureCodeDiff({ original, modified, originalLanguage, modifiedLanguage, dark }: {
  original: string; modified: string; originalLanguage: string; modifiedLanguage: string; dark: boolean;
}) {
  const { t } = useTranslation("structure");
  const viewport = useRef<HTMLDivElement>(null);

  /** 以可见容器为尺寸来源，防止编辑器内部的旧高度把末行推到裁切区域。 */
  const mount = useCallback<DiffOnMount>((instance) => {
    const host = viewport.current!;
    const layout = () => {
      const width = host.clientWidth;
      const height = host.clientHeight;
      if (width > 0 && height > 0) instance.layout({ width, height });
    };
    const resize = new ResizeObserver(layout);
    resize.observe(host);
    layout();
    const models = instance.getModel();
    instance.getModifiedEditor().onDidDispose(() => {
      resize.disconnect();
      // 先让 diff editor 解除模型关联，再释放模型，兼容 Monaco 0.55。
      queueMicrotask(() => {
        models?.original.dispose();
        models?.modified.dispose();
      });
    });
  }, []);

  return <div ref={viewport} className="relative min-h-0 min-w-0 flex-1 overflow-hidden rounded-md border" data-structure-code-diff>
    <div className="absolute inset-0"><DiffEditor
      height="100%"
      original={original}
      modified={modified}
      originalLanguage={originalLanguage}
      modifiedLanguage={modifiedLanguage}
      theme={dark ? REVIEW_DARK_THEME : REVIEW_LIGHT_THEME}
      beforeMount={registerReviewEditorThemes}
      onMount={mount}
      keepCurrentOriginalModel
      keepCurrentModifiedModel
      loading={<p className="p-3 text-xs text-muted-foreground" role="status">{t("loadingCodeDiff")}</p>}
      options={{
        readOnly: true,
        originalEditable: false,
        automaticLayout: false,
        renderSideBySide: false,
        ignoreTrimWhitespace: false,
        wordWrap: "on",
        diffWordWrap: "on",
        minimap: { enabled: false },
        fontFamily: "'JetBrains Mono Variable', monospace",
        fontSize: 12,
        lineHeight: 20,
        lineNumbersMinChars: 3,
        lineDecorationsWidth: 6,
        scrollBeyondLastLine: false,
        renderOverviewRuler: false,
        overviewRulerLanes: 0,
        renderLineHighlight: "none",
        contextmenu: false,
        stickyScroll: { enabled: false },
        padding: { top: 8, bottom: 8 },
        scrollbar: { alwaysConsumeMouseWheel: false },
        originalAriaLabel: t("before"),
        modifiedAriaLabel: t("after"),
      }}
    /></div>
  </div>;
}

import { useCallback, useEffect, useState } from "react";
import { DiffEditor, type DiffOnMount, type MonacoDiffEditor } from "@monaco-editor/react";
import { useTranslation } from "react-i18next";
import { configureLocalMonaco } from "@/infrastructure/local-monaco";
import { registerReviewEditorThemes, REVIEW_DARK_THEME, REVIEW_LIGHT_THEME } from "@/review/editor-theme";

configureLocalMonaco();

/** 在窄侧栏内直接比较两侧原文；长行仅视觉折行，不格式化或改写源码。 */
export function StructureCodeDiff({ original, modified, originalLanguage, modifiedLanguage, dark }: {
  original: string; modified: string; originalLanguage: string; modifiedLanguage: string; dark: boolean;
}) {
  const { t } = useTranslation("structure");
  const [editor, setEditor] = useState<MonacoDiffEditor>();
  const [height, setHeight] = useState(160);

  /** 先让 diff editor 解除模型关联，再释放本次创建的两侧模型，兼容 Monaco 0.55。 */
  const mount = useCallback<DiffOnMount>((instance) => {
    const models = instance.getModel();
    instance.getModifiedEditor().onDidDispose(() => queueMicrotask(() => {
      models?.original.dispose();
      models?.modified.dispose();
    }));
    setEditor(instance);
  }, []);

  /** 高度包含删除行和折行；短字段紧凑展示，长实现保留编辑器内滚动。 */
  useEffect(() => {
    if (!editor) return;
    const modifiedEditor = editor.getModifiedEditor();
    // 计入上下边框，避免短片段出现只有两像素可滚动的滚动条。
    const resize = () => setHeight(Math.max(80, Math.min(560, modifiedEditor.getContentHeight() + 2)));
    const content = modifiedEditor.onDidContentSizeChange(resize);
    const difference = editor.onDidUpdateDiff(resize);
    resize();
    return () => { content.dispose(); difference.dispose(); };
  }, [editor]);

  return <div className="max-h-[60vh] min-w-0 overflow-hidden rounded-md border" style={{ height }} data-structure-code-diff>
    <DiffEditor
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
        automaticLayout: true,
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
    />
  </div>;
}

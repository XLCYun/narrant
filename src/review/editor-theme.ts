import type { Monaco } from "@monaco-editor/react";

/** GitHub 浅色 Diff 的主题名；仅在编辑器边界替换应用的 light 偏好。 */
export const REVIEW_LIGHT_THEME = "narrant-github-light";

/** 按参考截图取色的 GitHub 深色 Diff 主题。 */
export const REVIEW_DARK_THEME = "narrant-github-dark";

/** 在编辑器创建前注册主题，让首次加载和运行时切换使用同一套配色。 */
export function registerReviewEditorThemes(monaco: Monaco): void {
  monaco.editor.defineTheme(REVIEW_LIGHT_THEME, {
    base: "vs",
    inherit: true,
    rules: [
      { token: "", foreground: "1f2328" },
      { token: "comment", foreground: "59636e" },
      { token: "keyword", foreground: "cf222e" },
      { token: "string", foreground: "0a3069" },
      { token: "number", foreground: "0550ae" },
      { token: "type", foreground: "953800" },
      { token: "delimiter", foreground: "1f2328" },
      { token: "delimiter.bracket", foreground: "1f2328" },
    ],
    colors: {
      "editor.background": "#ffffff",
      "editor.foreground": "#1f2328",
      "editorGutter.background": "#ffffff",
      "editorLineNumber.foreground": "#59636e",
      "editorLineNumber.activeForeground": "#1f2328",
      // 在白底上分别合成为 #dafbe1 / #ffebe9，透明度保留底层 decoration。
      "diffEditor.insertedLineBackground": "#b5f7c380",
      "diffEditor.removedLineBackground": "#ffd7d380",
      // 叠加在变更行上，强调实际改动的字符，而不是整行使用同一深色。
      "diffEditor.insertedTextBackground": "#7ee19580",
      "diffEditor.removedTextBackground": "#ffb1ad80",
      "diffEditorGutter.insertedLineBackground": "#aceebb",
      "diffEditorGutter.removedLineBackground": "#ffcecb",
      "diffEditor.diagonalFill": "#d1d9e0",
    },
  });
  monaco.editor.defineTheme(REVIEW_DARK_THEME, {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "", foreground: "f1f6fc" },
      { token: "identifier", foreground: "f1f6fc" },
      { token: "comment", foreground: "9298a2" },
      { token: "keyword", foreground: "ff7b72" },
      { token: "keyword.type", foreground: "79c0ff" },
      { token: "string", foreground: "a5d6ff" },
      { token: "number", foreground: "79c0ff" },
      { token: "type", foreground: "d2a8ff" },
      { token: "type.identifier", foreground: "d2a8ff" },
      { token: "function", foreground: "d2a8ff" },
      { token: "operator", foreground: "f1f6fc" },
      { token: "operator.reference", foreground: "79c0ff" },
      { token: "delimiter", foreground: "f1f6fc" },
      { token: "delimiter.bracket", foreground: "f1f6fc" },
    ],
    colors: {
      "editor.background": "#0c1117",
      "editor.foreground": "#f1f6fc",
      "editorGutter.background": "#0c1117",
      "editorLineNumber.foreground": "#9298a2",
      "editorLineNumber.activeForeground": "#f1f6fc",
      "editor.lineHighlightBorder": "#00000000",
      // 在深色底上合成为截图中的 #12261f / #26171d，保留装饰层透明叠加。
      "diffEditor.insertedLineBackground": "#183b2780",
      "diffEditor.removedLineBackground": "#401d2380",
      "diffEditor.insertedTextBackground": "#26623380",
      "diffEditor.removedTextBackground": "#82333580",
      "diffEditorGutter.insertedLineBackground": "#1c4429",
      "diffEditorGutter.removedLineBackground": "#542527",
      "diffEditor.diagonalFill": "#30363d",
      // 参考图中的括号与正文同色，不使用深色基主题的彩虹括号配色。
      "editorBracketHighlight.foreground1": "#f1f6fc",
      "editorBracketHighlight.foreground2": "#f1f6fc",
      "editorBracketHighlight.foreground3": "#f1f6fc",
      "editorBracketHighlight.foreground4": "#f1f6fc",
      "editorBracketHighlight.foreground5": "#f1f6fc",
      "editorBracketHighlight.foreground6": "#f1f6fc",
    },
  });
}

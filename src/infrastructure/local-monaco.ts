import { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import cssWorker from "monaco-editor/esm/vs/language/css/css.worker?worker";
import htmlWorker from "monaco-editor/esm/vs/language/html/html.worker?worker";
import tsWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";
import { createReviewRustLanguage } from "@/infrastructure/review-rust-language";

let configured = false;

/** 让 Monaco 及其 Worker 全部由 Vite 打进安装包，不访问默认 CDN。 */
export function configureLocalMonaco(): void {
  // 文件与结构视图共用同一加载器，避免重复注册 Rust 语言服务。
  if (configured) return;
  configured = true;
  self.MonacoEnvironment = {
    getWorker(_moduleId, label) {
      if (label === "json") return new jsonWorker();
      if (label === "css" || label === "scss" || label === "less") return new cssWorker();
      if (label === "html" || label === "handlebars" || label === "razor") return new htmlWorker();
      if (label === "typescript" || label === "javascript") return new tsWorker();
      return new editorWorker();
    },
  };
  loader.config({ monaco });
  // 继续按需加载 Rust，避免主题增强增加编辑器的首次启动体积。
  monaco.languages.registerTokensProviderFactory("rust", {
    async create() {
      const { language } = await import("monaco-editor/esm/vs/basic-languages/rust/rust.js");
      return createReviewRustLanguage(language);
    },
  });
  void loader.init();
}

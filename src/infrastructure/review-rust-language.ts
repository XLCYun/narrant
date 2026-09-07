import type { languages } from "monaco-editor";

/** 在内置 Rust 词法规则上区分调用名称与引用符号，供阅读器主题独立着色。 */
export function createReviewRustLanguage(base: languages.IMonarchLanguage): languages.IMonarchLanguage {
  return {
    ...base,
    tokenizer: {
      ...base.tokenizer,
      root: [
        [/[a-zA-Z_][\w]*(?=\s*\()/, {
          cases: {
            "@keywords": "keyword",
            "@typeKeywords": "keyword.type",
            "@default": "function",
          },
        }],
        [/&/, "operator.reference"],
        ...base.tokenizer.root,
      ],
    },
  };
}

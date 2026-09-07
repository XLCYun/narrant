import { afterAll, describe, expect, it } from "vitest";
import * as monaco from "monaco-editor";
import { language } from "monaco-editor/esm/vs/basic-languages/rust/rust.js";
import { createReviewRustLanguage } from "@/infrastructure/review-rust-language";

const languageId = "narrant-test-rust";
monaco.languages.register({ id: languageId });
const provider = monaco.languages.setMonarchTokensProvider(languageId, createReviewRustLanguage(language));
afterAll(() => provider.dispose());

/** 通过真实 Monarch 分词取得源码片段，验证高亮不会把变量、字符串或注释当成调用。 */
function fragments(source: string, type: string): string[] {
  const lines = source.split("\n");
  return monaco.editor.tokenize(source, languageId).flatMap((tokens, line) => tokens.flatMap((token, index) =>
    token.type === `${type}.rust`
      ? [lines[line].slice(token.offset, tokens[index + 1]?.offset ?? lines[line].length)]
      : [],
  ));
}

describe("Rust 阅读器语法高亮", () => {
  it("将调用名称与普通变量、关键字和引用符号分开", () => {
    const source = "let _ = babysit.clear_snapshot(&review_id);\nOk(map_presentation(presentation))\nif (ready) { run(); }";
    expect(fragments(source, "function")).toEqual(["clear_snapshot", "Ok", "map_presentation", "run"]);
    expect(fragments(source, "operator.reference")).toEqual(["&"]);
    expect(fragments(source, "keyword")).toEqual(["let", "if"]);
  });

  it("保留普通字符串、原始字符串和嵌套块注释的分词边界", () => {
    const source = '// fake()\nlet text = "fake() &x";\nlet raw = r#"fake()"#;\n/* outer()\n/* nested() */\nend() */\nreal();';
    expect(fragments(source, "function")).toEqual(["real"]);
    expect(fragments(source, "operator.reference")).toEqual([]);
  });
});

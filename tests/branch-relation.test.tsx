import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { BranchRelation } from "@/components/branch-relation";

afterEach(cleanup);

describe("Base Ref 与 Head Ref 关系", () => {
  it("默认从 Head 向 Base 展示合入方向", () => {
    render(<BranchRelation baseRef="refs/heads/main" headRef="refs/heads/release" />);

    const relation = screen.getByLabelText("将 Head 分支 release 合入 Base 分支 main");
    expect(relation.textContent).toBe("releasemain");
  });

  it("完整显示任意长度的分支名，并允许标签在可用宽度内换行", () => {
    const base = "team/a-very-long-base-branch-name-without-a-fixed-limit";
    const head = "feature/a-very-long-head-branch-name-without-a-fixed-limit";
    render(<div className="w-32"><BranchRelation baseRef={`refs/heads/${base}`} headRef={`refs/heads/${head}`} /></div>);

    for (const name of [base, head]) {
      const tag = screen.getByText(name);
      expect(tag).not.toHaveClass("truncate");
      expect(tag).not.toHaveClass("max-w-28");
      expect(tag).toHaveClass("break-all", "whitespace-normal");
      expect(tag).toHaveClass("border", "border-border", "bg-muted", "text-foreground");
      expect(tag).toHaveClass("hover:bg-muted");
      expect(tag).not.toHaveClass("hover:bg-muted-foreground/15");
    }
  });
});

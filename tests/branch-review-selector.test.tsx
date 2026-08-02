import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { BranchReviewSelector } from "@/components/branch-review-selector";
import { i18n } from "@/lib/i18n";

afterEach(() => cleanup());

describe("本地 Branch Selection", () => {
  it("按 Head 到 Base 的方向显示短分支名与默认来源", async () => {
    await i18n.changeLanguage("zh-CN");
    render(<BranchReviewSelector
      refs={["refs/heads/release", "refs/remotes/origin/main"]}
      currentRef="refs/heads/release"
      defaultRef="refs/remotes/origin/main"
      headRef="refs/heads/release"
      baseRef="refs/remotes/origin/main"
      action="start"
      onHeadChange={vi.fn()}
      onBaseChange={vi.fn()}
      onReload={vi.fn()}
      onSubmit={vi.fn()}
    />);

    expect(screen.getByLabelText("Head 分支")).toHaveTextContent("release");
    expect(screen.getByLabelText("Head 分支")).toHaveTextContent("当前分支");
    expect(screen.getByLabelText("Base 分支")).toHaveTextContent("origin/main");
    expect(screen.getByLabelText("Base 分支")).toHaveTextContent("默认分支");
    expect(screen.getByText("将 release 合入 origin/main")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "开始审核" }));
  });

  it("长分支名在各自选择器内收缩，不会覆盖相邻的 Base 分支", async () => {
    await i18n.changeLanguage("zh-CN");
    const longBranch = "codex/frontend-product-experience-spec-with-an-extra-long-suffix";
    render(<BranchReviewSelector
      refs={[`refs/heads/${longBranch}`, "refs/remotes/origin/main"]}
      currentRef={`refs/heads/${longBranch}`}
      defaultRef="refs/remotes/origin/main"
      headRef={`refs/heads/${longBranch}`}
      baseRef="refs/remotes/origin/main"
      action="start"
      onHeadChange={vi.fn()}
      onBaseChange={vi.fn()}
      onReload={vi.fn()}
      onSubmit={vi.fn()}
    />);

    const headTrigger = screen.getByLabelText("Head 分支");
    expect(headTrigger).toHaveClass("min-w-0", "overflow-hidden");
    expect(screen.getByText("当前分支")).toHaveClass("shrink-0");
  });

  it("Head 与 Base 相同时就地解释并禁用主操作", async () => {
    await i18n.changeLanguage("zh-CN");
    render(<BranchReviewSelector
      refs={["refs/heads/main"]}
      currentRef="refs/heads/main"
      defaultRef="refs/heads/main"
      headRef="refs/heads/main"
      baseRef="refs/heads/main"
      action="invalid"
      onHeadChange={vi.fn()}
      onBaseChange={vi.fn()}
      onReload={vi.fn()}
      onSubmit={vi.fn()}
    />);

    expect(screen.getByRole("status")).toHaveTextContent("Head 与 Base 不能是同一分支");
    expect(screen.getByRole("button", { name: "开始审核" })).toBeDisabled();
  });

  it("检查已有审核更新时显示 Loading 并延迟展示状态文字", async () => {
    await i18n.changeLanguage("zh-CN");
    render(<BranchReviewSelector
      refs={["refs/heads/release", "refs/remotes/origin/main"]}
      currentRef="refs/heads/release"
      defaultRef="refs/remotes/origin/main"
      headRef="refs/heads/release"
      baseRef="refs/remotes/origin/main"
      action="checking"
      showCheckingMessage
      onHeadChange={vi.fn()}
      onBaseChange={vi.fn()}
      onReload={vi.fn()}
      onSubmit={vi.fn()}
    />);

    expect(screen.getByRole("button", { name: "正在检查审核更新" })).toBeDisabled();
    expect(screen.getByText("正在检查审核内容是否有更新…")).toBeInTheDocument();
  });

  it("检测到新提交时解释更新对象并提供更新操作", async () => {
    await i18n.changeLanguage("zh-CN");
    render(<BranchReviewSelector
      refs={["refs/heads/release", "refs/remotes/origin/main"]}
      currentRef="refs/heads/release"
      defaultRef="refs/remotes/origin/main"
      headRef="refs/heads/release"
      baseRef="refs/remotes/origin/main"
      action="update"
      onHeadChange={vi.fn()}
      onBaseChange={vi.fn()}
      onReload={vi.fn()}
      onSubmit={vi.fn()}
    />);

    expect(screen.getByRole("status")).toHaveTextContent("检测到所选分支有新提交");
    expect(screen.getByRole("status")).toHaveTextContent("将使用最新的分支内容继续审核。");
    expect(screen.getByRole("button", { name: "更新并继续审核" })).toBeEnabled();
  });

  it("远程更新检查失败时允许继续现有审核且不暗示内容最新", async () => {
    await i18n.changeLanguage("zh-CN");
    render(<BranchReviewSelector
      refs={["refs/heads/release", "refs/remotes/origin/main"]}
      currentRef="refs/heads/release"
      defaultRef="refs/remotes/origin/main"
      headRef="refs/heads/release"
      baseRef="refs/remotes/origin/main"
      action="fallback"
      onHeadChange={vi.fn()}
      onBaseChange={vi.fn()}
      onReload={vi.fn()}
      onSubmit={vi.fn()}
    />);

    expect(screen.getByRole("status")).toHaveTextContent("无法检查 origin/main 的远程更新");
    expect(screen.getByRole("status")).toHaveTextContent("当前审核版本可能不是最新");
    expect(screen.getByRole("button", { name: "继续现有审核" })).toBeEnabled();
  });

  it("仅选择本地分支时不把更新检查失败误报为远程失败", async () => {
    await i18n.changeLanguage("zh-CN");
    render(<BranchReviewSelector
      refs={["refs/heads/release", "refs/heads/main"]}
      currentRef="refs/heads/release"
      defaultRef="refs/heads/main"
      headRef="refs/heads/release"
      baseRef="refs/heads/main"
      action="fallback"
      onHeadChange={vi.fn()}
      onBaseChange={vi.fn()}
      onReload={vi.fn()}
      onSubmit={vi.fn()}
    />);

    expect(screen.getByRole("status")).toHaveTextContent("无法检查分支更新");
    expect(screen.getByRole("status")).not.toHaveTextContent("远程更新");
  });

  it("重新读取分支期间保留选择并把刷新操作切换为 Loading", async () => {
    await i18n.changeLanguage("zh-CN");
    render(<BranchReviewSelector
      refs={["refs/heads/release", "refs/remotes/origin/main"]}
      currentRef="refs/heads/release"
      defaultRef="refs/remotes/origin/main"
      headRef="refs/heads/release"
      baseRef="refs/remotes/origin/main"
      action="start"
      reloading
      onHeadChange={vi.fn()}
      onBaseChange={vi.fn()}
      onReload={vi.fn()}
      onSubmit={vi.fn()}
    />);

    expect(screen.getByLabelText("Head 分支")).toHaveTextContent("release");
    expect(screen.getByRole("button", { name: "正在重新读取分支" })).toBeDisabled();
  });
});

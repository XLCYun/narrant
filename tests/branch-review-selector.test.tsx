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
    expect(headTrigger).toHaveClass("max-w-full", "overflow-hidden");
    expect(screen.getByText("当前分支")).toHaveClass("h-5", "shrink-0", "leading-none");
  });

  it("打开 Head 选择器后可以搜索并选择已有分支", async () => {
    await i18n.changeLanguage("zh-CN");
    const onHeadChange = vi.fn();
    render(<BranchReviewSelector
      refs={[
        "refs/heads/main",
        "refs/heads/codex/issue-21-repository-entries",
        "refs/heads/codex/issue-11-system-theme",
        "refs/remotes/origin/main",
      ]}
      currentRef="refs/heads/main"
      defaultRef="refs/remotes/origin/main"
      headRef="refs/heads/main"
      baseRef="refs/remotes/origin/main"
      action="start"
      onHeadChange={onHeadChange}
      onBaseChange={vi.fn()}
      onReload={vi.fn()}
      onSubmit={vi.fn()}
    />);

    fireEvent.click(screen.getByLabelText("Head 分支"));
    const search = await screen.findByPlaceholderText("搜索分支…");
    fireEvent.change(search, { target: { value: "issue-21" } });

    expect(screen.getByText("codex/issue-21-repository-entries")).toBeInTheDocument();
    expect(screen.queryByText("codex/issue-11-system-theme")).not.toBeInTheDocument();
    fireEvent.click(screen.getByText("codex/issue-21-repository-entries"));
    expect(onHeadChange).toHaveBeenCalledWith("refs/heads/codex/issue-21-repository-entries");
  });

  it("搜索无结果时解释原因，关闭后重新打开会恢复完整列表", async () => {
    await i18n.changeLanguage("zh-CN");
    render(<BranchReviewSelector
      refs={["refs/heads/main", "refs/heads/release", "refs/remotes/origin/main"]}
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

    fireEvent.click(screen.getByLabelText("Head 分支"));
    const search = await screen.findByPlaceholderText("搜索分支…");
    fireEvent.change(search, { target: { value: "missing" } });
    expect(screen.getByText("没有匹配的分支")).toBeInTheDocument();

    fireEvent.keyDown(search, { key: "Escape" });
    fireEvent.click(screen.getByLabelText("Head 分支"));
    expect(await screen.findByText("main")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("搜索分支…")).toHaveValue("");
  });

  it("选择器聚焦时直接输入字符会打开弹层并开始搜索", async () => {
    await i18n.changeLanguage("zh-CN");
    render(<BranchReviewSelector
      refs={["refs/heads/main", "refs/heads/issue-21", "refs/remotes/origin/main"]}
      currentRef="refs/heads/main"
      defaultRef="refs/remotes/origin/main"
      headRef="refs/heads/main"
      baseRef="refs/remotes/origin/main"
      action="start"
      onHeadChange={vi.fn()}
      onBaseChange={vi.fn()}
      onReload={vi.fn()}
      onSubmit={vi.fn()}
    />);

    fireEvent.keyDown(screen.getByLabelText("Head 分支"), { key: "z" });
    expect(await screen.findByPlaceholderText("搜索分支…")).toHaveValue("z");
  });

  it("按 Space 打开选择器时不会把空格作为搜索词", async () => {
    await i18n.changeLanguage("zh-CN");
    render(<BranchReviewSelector
      refs={["refs/heads/main", "refs/heads/release", "refs/remotes/origin/main"]}
      currentRef="refs/heads/main"
      defaultRef="refs/remotes/origin/main"
      headRef="refs/heads/main"
      baseRef="refs/remotes/origin/main"
      action="start"
      onHeadChange={vi.fn()}
      onBaseChange={vi.fn()}
      onReload={vi.fn()}
      onSubmit={vi.fn()}
    />);

    const trigger = screen.getByLabelText("Head 分支");
    fireEvent.keyDown(trigger, { key: " " });
    fireEvent.keyUp(trigger, { key: " " });
    expect(await screen.findByPlaceholderText("搜索分支…")).toHaveValue("");
    expect(screen.getByText("release")).toBeInTheDocument();
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

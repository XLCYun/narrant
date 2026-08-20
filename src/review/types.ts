import type { PullRequestMetadata } from "@/lib/review-types";
import type { MonacoDiffEditor } from "@monaco-editor/react";

/** Review Reader 的稳定 peer view。 */
export type ReviewView = "overview" | "file-changes";

/** Monaco Diff 阅读器支持的持久化展示布局。 */
export type ReviewDiffMode = "side-by-side" | "inline";

/** Reader 可控滚动容器的进程内导航位置。 */
export type ReviewReaderScrollPositions = {
  overview: number;
  fileTree: number;
  content: number;
};

/** Monaco Diff 官方 saveViewState 返回的可恢复视口与光标状态。 */
export type ReviewDiffViewState = NonNullable<ReturnType<MonacoDiffEditor["saveViewState"]>>;

/** File Change 在文件树与内容视图中共享的状态。 */
export type ChangeStatus = "added" | "modified" | "deleted" | "renamed";

/** Review Snapshot 中一个 File Change 的稳定摘要。 */
export type FileChangeSummary = {
  path: string;
  oldPath: string | null;
  status: ChangeStatus;
};

/** 当前选中 File Change 的可展示内容。 */
export type FileChangeContent = FileChangeSummary & {
  contentKind: "text" | "binary" | "submodule" | "symlink";
  beforeSize: number | null;
  afterSize: number | null;
  before: string | null;
  after: string | null;
};

/** Review Reader 当前呈现的 immutable Review Snapshot。 */
export type ReviewSnapshot = {
  reviewId: string;
  snapshotId: string;
  baseRef: string;
  baseRevision: string;
  comparisonBase: string;
  headRef: string;
  headRevision: string;
  fileChanges: FileChangeSummary[];
  pullRequest?: PullRequestMetadata;
};

/** 当前 Review 与 Review Snapshot 的运行时读取 identity。 */
export type ReviewIdentity = {
  reviewId: string;
  snapshotId: string;
};

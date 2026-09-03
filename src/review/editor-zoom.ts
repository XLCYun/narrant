export const REVIEW_EDITOR_ZOOM_LEVEL_KEY = "narrant.review.editorZoomLevel";

/** 读取经过 Monaco 技术边界校验的全局 Reader 字体缩放级别。 */
export function readReviewEditorZoomLevel(): number {
  const rawLevel = localStorage.getItem(REVIEW_EDITOR_ZOOM_LEVEL_KEY);
  if (rawLevel === null) return 0;
  const level = Number(rawLevel);
  return Number.isInteger(level) && level >= -5 && level <= 20 ? level : 0;
}

/** 保存 Monaco 已经钳制到技术边界内的全局 Reader 字体缩放级别。 */
export function saveReviewEditorZoomLevel(level: number): void {
  localStorage.setItem(REVIEW_EDITOR_ZOOM_LEVEL_KEY, String(level));
}

/** 在没有文本 Diff 时预先调整字号偏好，供下一个 Monaco 实例恢复。 */
export function changeReviewEditorZoomLevel(direction: -1 | 0 | 1): number {
  const current = readReviewEditorZoomLevel();
  const next = direction === 0 ? 0 : Math.min(20, Math.max(-5, current + direction));
  saveReviewEditorZoomLevel(next);
  return next;
}

/** Peek 文案独立维护，由 structure namespace 一并注册。 */
export const peekZh = {
  title: "源码预览", sources: "源码位置", before: "原位置", after: "新位置", declaration: "声明", implementation: "实现", documentation: "文档", occurrence: "发生位置",
  target: "查看目标定义", close: "关闭源码预览", openFileChange: "在文件改动中打开", unchanged: "未变更", loading: "正在读取完整源码…", retry: "重试", external: "仓库外目标没有已索引的本地定义。", noLocation: "当前分析没有可用的源码位置。",
  unavailable: "{{side}}源码不可用：{{message}}", readableOnly: "仅显示可读侧的完整源码；无法比较两侧。", absent: "{{side}}在此 Snapshot 中不存在。", fullFile: "完整文件", line: "第 {{line}} 行",
};

/** 英文保持来源、缺侧和不可用的同一语义。 */
export const peekEn: Record<keyof typeof peekZh, string> = {
  title: "Source Peek", sources: "Source locations", before: "Before", after: "After", declaration: "Declaration", implementation: "Implementation", documentation: "Documentation", occurrence: "Occurrence",
  target: "Open target definition", close: "Close source Peek", openFileChange: "Open in File Changes", unchanged: "Unchanged", loading: "Reading complete source…", retry: "Retry", external: "This external target has no indexed local definition.", noLocation: "This analysis has no available source location.",
  unavailable: "{{side}} source unavailable: {{message}}", readableOnly: "Showing the readable side in full; comparison is unavailable.", absent: "{{side}} does not exist in this Snapshot.", fullFile: "Full file", line: "Line {{line}}",
};

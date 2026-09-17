/** 文件路径由分析器的 file 容器明确提供，不从不透明节点 ID 推断。 */
export type DirectoryFile = { objectId: string; path: string };
/** 目录仅供展示；路径是折叠身份，文件列表包含当前显示范围内的所有后代文件。 */
export type DirectoryGroup = { path: string; label: string; parentPath?: string; files: DirectoryFile[] };

/** 只组织有可见对象的文件，压缩没有直属文件的连续单子目录。 */
export function groupStructureFiles(files: DirectoryFile[]): DirectoryGroup[] {
  /** 中间目录树保留直属文件，保证压缩时不会跨过实际分叉或文件归属。 */
  type Branch = { path: string; files: DirectoryFile[]; children: Map<string, Branch> };
  const root: Branch = { path: "", files: [], children: new Map() };
  for (const file of [...files].sort((a, b) => a.path.localeCompare(b.path) || a.objectId.localeCompare(b.objectId))) {
    const parts = file.path.split("/");
    if (parts.some((part) => !part || part === "." || part === "..")) continue;
    let parent = root;
    for (const name of parts.slice(0, -1)) {
      let child = parent.children.get(name);
      if (!child) {
        child = { path: parent.path ? `${parent.path}/${name}` : name, files: [], children: new Map() };
        parent.children.set(name, child);
      }
      parent = child;
    }
    parent.files.push(file);
  }
  const groups: DirectoryGroup[] = [];
  /** 自顶向下生成稳定分组，再收集后代文件，供目录详情和折叠摘要复用。 */
  const visit = (branch: Branch, parentPath?: string): DirectoryGroup => {
    while (branch.files.length === 0 && branch.children.size === 1) branch = [...branch.children.values()][0];
    const group: DirectoryGroup = {
      path: branch.path, label: parentPath ? branch.path.slice(parentPath.length + 1) : branch.path,
      parentPath, files: [...branch.files],
    };
    groups.push(group);
    for (const child of branch.children.values()) group.files.push(...visit(child, group.path).files);
    group.files.sort((a, b) => a.path.localeCompare(b.path) || a.objectId.localeCompare(b.objectId));
    return group;
  };
  for (const branch of root.children.values()) visit(branch);
  return groups;
}

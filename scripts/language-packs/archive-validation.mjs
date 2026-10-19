/** 校验可信归档路径和文件种类，拒绝链接、绝对路径与父目录跳转。 */
export function validateArchive(names, listing) {
  const entries = names.trim().split('\n');
  const prefix = entries[0]?.split('/')[0];
  if (!prefix || entries.some(name => name.includes('\\') || name.startsWith('/')
    || name.split('/').includes('..') || name.split('/')[0] !== prefix)
    || listing.trim().split('\n').some(line => !['-', 'd'].includes(line[0]))) {
    throw new Error('分析工具归档含不安全的路径或文件类型');
  }
}

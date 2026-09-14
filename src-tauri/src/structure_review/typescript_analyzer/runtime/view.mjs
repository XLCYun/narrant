import fs from 'node:fs';
import path from 'node:path';

export const sourcePattern = /\.(?:[cm]?[jt]sx?)$/i;
const excluded = new Set(['node_modules', '.git', '.tools', '.narrant-typescript']);

/** 路径必须在指定目录内；先检查词法边界，再检查已存在路径的真实边界。 */
export function within(root, candidate) {
  const relative = path.relative(root, candidate);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
}

/** 将原始侧的绝对路径映射到视图，拒绝仓库外或隐藏依赖中的源码。 */
export function mapped(context, original, metadata = false) {
  original = path.resolve(original);
  if (!within(context.root, original)) throw new Error(`仓库外路径：${original}`);
  const relative = path.relative(context.root, original);
  const real = fs.existsSync(original) ? fs.realpathSync(original) : original;
  if (!within(context.root, real)) throw new Error(`符号链接越界：${original}`);
  if (!metadata && path.relative(context.root, real).split(path.sep).some(part => excluded.has(part))) {
    throw new Error(`不允许的依赖源码：${original}`);
  }
  return path.join(context.view, relative);
}

/** 复制纯数据与第一方源码，不读取任何业务依赖源码，也不执行配置或包脚本。 */
export function copyRepository(context) {
  const sources = [];
  const manifests = [];
  const metadata = [];
  const links = [];
  /** 仅记录 node_modules 中回到第一方目录的链接；其他依赖完全不复制。 */
  function workspaceLinks(directory) {
    if (!fs.existsSync(directory)) return;
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const candidate = path.join(directory, entry.name);
      if (entry.name.startsWith('@') && entry.isDirectory()) { workspaceLinks(candidate); continue; }
      if (!entry.isSymbolicLink()) continue;
      const target = fs.realpathSync(candidate);
      if (within(context.root, target) && !path.relative(context.root, target).split(path.sep).some(part => excluded.has(part))) {
        links.push({ link: mapped(context, candidate, true), target: mapped(context, target) });
      }
    }
  }
  /** 递归只遍历真实目录；目录符号链接必须作为工作区映射明确处理。 */
  function visit(directory) {
    workspaceLinks(path.join(directory, 'node_modules'));
    for (const entry of fs.readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      if (excluded.has(entry.name) || (entry.name === 'target' && fs.existsSync(path.join(directory, 'Cargo.toml')))) continue;
      const original = path.join(directory, entry.name);
      if (entry.isDirectory()) { visit(original); continue; }
      if (!sourcePattern.test(entry.name) && !/\.jsonc?$/i.test(entry.name)) continue;
      const target = mapped(context, original);
      if (!fs.statSync(original).isFile()) throw new Error(`非普通输入文件：${original}`);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      const content = fs.readFileSync(original, 'utf8');
      fs.writeFileSync(target, content);
      if (sourcePattern.test(entry.name)) sources.push({ original, view: target });
      if (/\.jsonc?$/i.test(entry.name)) metadata.push({ original, view: target });
      if (entry.name === 'package.json') {
        manifests.push(original);
        fs.writeFileSync(target, JSON.stringify(sanitizeManifest(context, original, JSON.parse(content)), null, 2));
      }
    }
  }
  visit(context.root);
  /** 将 workspace 声明的简单 glob 转成完整路径匹配器，支持 * 和 **。 */
  function workspacePattern(pattern) {
    if (typeof pattern !== 'string') return null;
    const escaped = pattern.replaceAll('\\', '/').replace(/[.+?^${}()|[\]\\]/g, '\\$&');
    return new RegExp(`^${escaped.replaceAll('**', '\u0000').replaceAll('*', '[^/]*').replaceAll('\u0000', '.*')}/?$`);
  }
  const rootPackage = manifests.includes(path.join(context.root, 'package.json'))
    ? JSON.parse(fs.readFileSync(path.join(context.root, 'package.json'), 'utf8')) : {};
  let patterns = rootPackage.workspaces?.packages ?? rootPackage.workspaces ?? [];
  if (!Array.isArray(patterns)) throw new Error('workspaces 必须为列表');
  const pnpm = path.join(context.root, 'pnpm-workspace.yaml');
  if (fs.existsSync(pnpm)) {
    // 不引入 YAML 执行器；只接受 packages 下的字符串列表，其余高级形式明确提示。
    const lines = fs.readFileSync(pnpm, 'utf8').match(/^packages:\s*\r?\n((?:[ \t]+.*\r?\n?)*)/m)?.[1] ?? '';
    patterns = [...patterns, ...Array.from(lines.matchAll(/^\s*-\s*['"]?([^'"\r\n]+?)['"]?\s*$/gm), match => match[1])];
    if (!lines || patterns.length === 0) context.note(pnpm, '无法读取 pnpm workspace 字符串列表；仅恢复已有第一方链接');
  }
  const matchers = patterns.map(pattern => ({ exclude: pattern.startsWith('!'), matcher: workspacePattern(pattern.replace(/^!/, '')) }));
  for (const original of manifests) {
    const metadata = JSON.parse(fs.readFileSync(original, 'utf8'));
    const directory = path.dirname(original);
    const relative = path.relative(context.root, directory).split(path.sep).join('/');
    if (!metadata.name || !matchers.some(({ exclude, matcher }) => !exclude && matcher?.test(relative))
      || matchers.some(({ exclude, matcher }) => exclude && matcher?.test(relative))) continue;
    if (!/^(?:@[\w.-]+\/)?[\w.-]+$/.test(metadata.name)) throw new Error('工作区包名称无效');
    links.push({ link: path.join(context.view, 'node_modules', metadata.name), target: mapped(context, directory) });
  }
  for (const { link, target } of links) {
    if (!fs.existsSync(target)) continue;
    fs.mkdirSync(path.dirname(link), { recursive: true });
    if (!fs.existsSync(link)) fs.symlinkSync(target, link, process.platform === 'win32' ? 'junction' : 'dir');
  }
  return { sources, manifests, metadata };
}

/** 包模块元数据保留解析结构，路径限定于原始侧；删除脚本避免被误当成可执行上下文。 */
function sanitizeManifest(context, original, manifest) {
  if (!manifest || typeof manifest !== 'object' || Array.isArray(manifest)) throw new Error(`无效包元数据：${original}`);
  delete manifest.scripts;
  const base = path.dirname(original);
  /** 保留正常相对入口，只重写绝对仓库路径；第三方包 specifier 不展开为源码。 */
  function target(value, relative = false) {
    if (typeof value === 'string') {
      if (relative || value.startsWith('.') || path.isAbsolute(value)) {
        const controlled = mapped(context, path.resolve(base, value), true);
        return path.isAbsolute(value) ? controlled : value;
      }
      return value;
    }
    if (Array.isArray(value)) return value.map(item => target(item, relative));
    if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, target(item, relative)]));
    if (value === null) return value;
    throw new Error(`无效模块入口：${original}`);
  }
  for (const key of ['main', 'module', 'types', 'typings', 'typesVersions']) if (manifest[key] !== undefined) manifest[key] = target(manifest[key], true);
  for (const key of ['exports', 'imports']) if (manifest[key] !== undefined) manifest[key] = target(manifest[key]);
  return manifest;
}

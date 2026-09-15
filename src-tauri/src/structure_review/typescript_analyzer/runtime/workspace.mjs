import fs from 'node:fs';
import path from 'node:path';

export const sourcePattern = /\.(?:[cm]?[jt]sx?)$/i;
const excluded = new Set(['node_modules', '.git', '.tools', '.narrant-typescript']);

/** 检查路径归属，不以字符串前缀混淆相邻目录。 */
export function within(root, candidate) {
  const relative = path.relative(root, candidate);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
}

/** 仅读取 Git workspace 内的路径；不存在的配置输出检查最近现存祖先。 */
export function checkedWorkspacePath(root, candidate) {
  candidate = path.resolve(candidate);
  if (!within(root, candidate)) throw new Error(`仓库外路径：${candidate}`);
  let ancestor = candidate;
  while (!fs.existsSync(ancestor) && ancestor !== root) ancestor = path.dirname(ancestor);
  if (!within(root, fs.realpathSync(ancestor))) throw new Error(`符号链接越界：${candidate}`);
  return candidate;
}

/** 与 Rust 的工作区约定一致，排除工具目录和 Cargo 生成目录，保留普通 target 源目录。 */
function firstParty(root, file) {
  if (!within(root, file)) return false;
  if (path.relative(root, file).split(path.sep).some(part => excluded.has(part))) return false;
  let directory = file;
  while (within(root, directory)) {
    if (path.basename(directory) === 'target' && fs.existsSync(path.join(path.dirname(directory), 'Cargo.toml'))) return false;
    if (directory === root) break;
    directory = path.dirname(directory);
  }
  return true;
}

/** 保留包的模块契约，仅校验路径范围，不改写入口、不运行 scripts。 */
function validateManifest(root, file, manifest) {
  if (!manifest || typeof manifest !== 'object' || Array.isArray(manifest)) throw new Error(`无效包元数据：${file}`);
  /** 条件入口和 typesVersions 逐层检查，裸包名留给语言服务解析。 */
  function target(value, relative = false) {
    if (typeof value === 'string') {
      if (relative || value.startsWith('.') || path.isAbsolute(value)) checkedWorkspacePath(root, path.resolve(path.dirname(file), value));
    } else if (Array.isArray(value)) value.forEach(item => target(item, relative));
    else if (value && typeof value === 'object') Object.values(value).forEach(item => target(item, relative));
    else if (value !== null) throw new Error(`无效模块入口：${file}`);
  }
  for (const key of ['main', 'module', 'types', 'typings', 'typesVersions']) if (manifest[key] !== undefined) target(manifest[key], true);
  for (const key of ['exports', 'imports']) if (manifest[key] !== undefined) target(manifest[key]);
  return manifest;
}

/** 第一方目录清单和模块位置只保存在内存，不复制文件、不创建链接、不写入 workspace。 */
export function createWorkspace(input) {
  const root = fs.realpathSync(input.root);
  const runtime = fs.realpathSync(input.runtime);
  const sources = new Set();
  const metadata = new Set();
  const links = new Map();
  const overlays = new Map();
  const manifests = [];
  const notes = [];
  /** 已有 workspace 链接只恢复到同一仓库中的第一方目标。 */
  function existingLinks(directory) {
    if (!fs.existsSync(directory)) return;
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.name.startsWith('@') && entry.isDirectory()) { existingLinks(file); continue; }
      if (!entry.isSymbolicLink()) continue;
      const target = fs.realpathSync(file);
      if (firstParty(root, target)) links.set(file, target);
    }
  }
  /** 原始文件保留唯一真实路径，语义查询和 Rust 均读取同一 Git 签出。 */
  function visit(directory) {
    existingLinks(path.join(directory, 'node_modules'));
    for (const entry of fs.readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const file = path.join(directory, entry.name);
      if (!firstParty(root, file)) continue;
      if (entry.isDirectory()) { visit(file); continue; }
      if (!sourcePattern.test(file) && !/\.jsonc?$/i.test(file)) continue;
      checkedWorkspacePath(root, file);
      const canonical = fs.realpathSync(file);
      if (!firstParty(root, canonical)) throw new Error(`不允许的依赖源码：${file}`);
      if (!fs.statSync(canonical).isFile()) throw new Error(`非普通输入文件：${file}`);
      if (sourcePattern.test(canonical)) sources.add(canonical);
      else metadata.add(canonical);
      if (entry.name === 'package.json') manifests.push(canonical);
    }
  }
  visit(root);
  /** 有界说明跟随当前会话，不借助全局文件或持久化缓存。 */
  function note(file, message) {
    if (notes.length < 200) notes.push({ file, message: message.slice(0, 4096) });
  }
  const rootManifest = path.join(root, 'package.json');
  const packageData = manifests.includes(rootManifest) ? JSON.parse(fs.readFileSync(rootManifest, 'utf8')) : {};
  let patterns = packageData.workspaces?.packages ?? packageData.workspaces ?? [];
  if (!Array.isArray(patterns)) throw new Error('workspaces 必须为列表');
  const pnpm = path.join(root, 'pnpm-workspace.yaml');
  if (fs.existsSync(pnpm)) {
    const lines = fs.readFileSync(pnpm, 'utf8').match(/^packages:\s*\r?\n((?:[ \t]+.*\r?\n?)*)/m)?.[1] ?? '';
    patterns = [...patterns, ...Array.from(lines.matchAll(/^\s*-\s*['"]?([^'"\r\n]+?)['"]?\s*$/gm), match => match[1])];
    if (!lines || patterns.length === 0) note(pnpm, '无法读取 pnpm workspace 字符串列表；仅使用已有第一方链接');
  }
  /** 保留原有简单 workspace glob，不猜测包 exports 的源码入口。 */
  function matcher(pattern) {
    if (typeof pattern !== 'string') throw new Error('workspace 模式必须为字符串');
    const escaped = pattern.replace(/^!/, '').replaceAll('\\', '/').replace(/[.+?^${}()|[\]\\]/g, '\\$&');
    return { exclude: pattern.startsWith('!'), test: new RegExp(`^${escaped.replaceAll('**', '\u0000').replaceAll('*', '[^/]*').replaceAll('\u0000', '.*')}/?$`) };
  }
  const matchers = patterns.map(matcher);
  for (const manifest of manifests) {
    const data = validateManifest(root, manifest, JSON.parse(fs.readFileSync(manifest, 'utf8')));
    const directory = path.dirname(manifest);
    const relative = path.relative(root, directory).split(path.sep).join('/');
    if (!data.name || !matchers.some(item => !item.exclude && item.test.test(relative))
      || matchers.some(item => item.exclude && item.test.test(relative))) continue;
    if (!/^(?:@[\w.-]+\/)?[\w.-]+$/.test(data.name)) throw new Error('工作区包名称无效');
    links.set(path.join(root, 'node_modules', data.name), directory);
  }
  for (const name of ['@types/node', 'undici-types']) {
    const virtual = path.join(root, 'node_modules', name);
    if (links.has(virtual)) throw new Error(`运行时类型名称被第一方包占用：${name}`);
    links.set(virtual, path.join(runtime, name));
  }
  // Node 声明的依赖从同一固定工具目录解析，不另建 node_modules 物理副本。
  links.set(path.join(runtime, 'node_modules/undici-types'), path.join(runtime, 'undici-types'));
  return { root, runtime, sources, metadata, links, overlays, notes, note, policyError: input.policy_error };
}

/** 给 TypeScript 的文件读取接口限定上下文，所有真实源码仍从 Git workspace 原位读取。 */
export function workspaceSystem(context, ts) {
  const native = { ...ts.sys };
  const libraries = ['typescript/lib', '@types/node', 'undici-types'].map(name => path.join(context.runtime, name));
  const links = [...context.links].sort((a, b) => b[0].length - a[0].length);
  /** 模块别名只转换读取位置，不创建文件或符号链接。 */
  function resolve(file) {
    file = path.resolve(file);
    for (const [alias, target] of links) {
      if (within(alias, file)) { file = path.join(target, path.relative(alias, file)); break; }
    }
    return native.realpath?.(file) ?? file;
  }
  /** 明确越界源码是分析错误，普通缺失业务依赖仍留给 tsserver 产生诊断。 */
  function permitted(file) {
    const canonical = resolve(file);
    if (context.overlays.has(canonical) || context.sources.has(canonical) || context.metadata.has(canonical)) return true;
    if (libraries.some(root => within(root, canonical)) && (/\.d\.[cm]?ts$/i.test(canonical) || /\.json$/i.test(canonical))) return true;
    if (!within(context.root, canonical) && !libraries.some(root => within(root, canonical))
      && sourcePattern.test(canonical) && native.fileExists(canonical)) {
      const message = `服务引入未授权源码：${canonical}`;
      if (context.policyError && !fs.existsSync(context.policyError)) fs.writeFileSync(context.policyError, message);
    }
    return false;
  }
  /** 合成的包路径仅存在于读取接口；目录本身不会落盘。 */
  function directoryExists(directory) {
    directory = path.resolve(directory);
    const canonical = resolve(directory);
    if (libraries.some(root => within(root, canonical) || within(canonical, root))) return native.directoryExists(canonical);
    if ([...context.links.keys()].some(alias => within(directory, alias))) return true;
    return firstParty(context.root, canonical) && native.directoryExists(canonical);
  }
  /** 为 TypeScript 原生 glob 算法提供经过范围筛选的目录条目。 */
  function entries(directory) {
    const canonical = resolve(directory);
    const files = new Set();
    const directories = new Set();
    if (directoryExists(directory) && fs.existsSync(canonical)) {
      for (const entry of fs.readdirSync(canonical, { withFileTypes: true })) {
        const child = path.join(directory, entry.name);
        if (entry.isDirectory() && directoryExists(child)) directories.add(entry.name);
        else if (permitted(child) && native.fileExists(resolve(child))) files.add(entry.name);
      }
    }
    for (const alias of context.links.keys()) {
      const relative = path.relative(directory, alias);
      if (relative && within(directory, alias)) directories.add(relative.split(path.sep)[0]);
    }
    return { files: [...files].sort(), directories: [...directories].sort() };
  }
  return {
    ...native,
    getCurrentDirectory: () => context.root,
    getExecutingFilePath: () => path.join(context.runtime, 'typescript/lib/tsserver.js'),
    fileExists: file => permitted(file) && (context.overlays.has(resolve(file)) || native.fileExists(resolve(file))),
    readFile: (file, encoding) => permitted(file) ? context.overlays.get(resolve(file)) ?? native.readFile(resolve(file), encoding) : undefined,
    directoryExists,
    getDirectories: directory => entries(directory).directories,
    readDirectory: (directory, extensions, excludes, includes, depth) => ts.matchFiles(directory, extensions, excludes, includes,
      native.useCaseSensitiveFileNames, context.root, depth, entries, resolve),
    realpath: resolve,
    // Git workspace 由 Review lease 管理；分析结束仍通过原始源码校验检测外部变化。
    watchFile: () => ({ close() {} }),
    watchDirectory: () => ({ close() {} }),
    writeFile() { throw new Error('TypeScript 分析不能写入 Git workspace'); },
    createDirectory() { throw new Error('TypeScript 分析不能创建 workspace 目录'); },
  };
}

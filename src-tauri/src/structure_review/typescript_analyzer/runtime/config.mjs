import fs from 'node:fs';
import path from 'node:path';
import { within, mapped } from './view.mjs';

/** 固定 TS 仅用于 JSONC 配置解析；不创建 Program、checker 或抽取图。 */
export function prepareConfigurations(context, ts, sources) {
  const done = new Set();
  const configs = new Set();
  const pathOptions = ['baseUrl', 'rootDir', 'outDir', 'declarationDir', 'tsBuildInfoFile'];
  const listPathOptions = ['rootDirs', 'typeRoots'];
  /** 把配置中的路径转成视图绝对路径，含不存在的输出目录也不能越界。 */
  function rewrite(value, base) {
    if (typeof value !== 'string') throw new Error('配置路径必须为字符串');
    if (within(context.view, path.resolve(base, value))) return path.resolve(base, value);
    return mapped(context, path.resolve(base, value), true);
  }
  /** 解析纯 JSONC extends；只读取包配置元数据，禁止 JS 和任意外部文件。 */
  function resolveExtends(request, directory) {
    if (typeof request !== 'string') throw new Error('extends 必须为字符串或字符串列表');
    const attempts = [];
    if (request.startsWith('.') || path.isAbsolute(request)) attempts.push(path.resolve(directory, request));
    else {
      let ancestor = directory;
      while (within(context.root, ancestor)) {
        attempts.push(path.join(ancestor, 'node_modules', request));
        if (ancestor === context.root) break;
        ancestor = path.dirname(ancestor);
      }
    }
    for (const candidate of attempts) {
      mapped(context, candidate, true);
      const candidates = [candidate, `${candidate}.json`, path.join(candidate, 'tsconfig.json')];
      const manifest = path.join(candidate, 'package.json');
      if (fs.existsSync(manifest)) {
        mapped(context, manifest, true);
        const packageData = JSON.parse(fs.readFileSync(manifest, 'utf8'));
        if (typeof packageData.tsconfig === 'string') candidates.unshift(path.resolve(candidate, packageData.tsconfig));
      }
      for (const file of candidates) {
        if (fs.existsSync(file) && fs.statSync(file).isFile()) {
          if (!/\.jsonc?$/.test(file)) throw new Error(`不执行配置代码：${file}`);
          mapped(context, file, true);
          return file;
        }
      }
    }
    context.note(directory, `缺失配置 extends：${request}`);
    return null;
  }
  /** 保留每层 extends/references；只剥离插件入口和改写路径。 */
  function processConfig(original) {
    original = path.resolve(original);
    const destination = mapped(context, original, true);
    if (done.has(original)) return destination;
    done.add(original);
    const parsed = ts.parseConfigFileTextToJson(original, fs.readFileSync(original, 'utf8'));
    if (parsed.error || !parsed.config || typeof parsed.config !== 'object' || Array.isArray(parsed.config)) {
      throw new Error(`无效配置 JSONC：${original}`);
    }
    const config = parsed.config;
    const base = path.dirname(original);
    const options = config.compilerOptions ??= {};
    if (typeof options !== 'object' || Array.isArray(options)) throw new Error('compilerOptions 必须为对象');
    delete options.plugins;
    delete config.compileOnSave;
    config.typeAcquisition = { enable: false, include: [], exclude: [] };
    for (const key of pathOptions) if (options[key] !== undefined) options[key] = rewrite(options[key], base);
    for (const key of listPathOptions) if (options[key] !== undefined) options[key] = options[key].map(value => rewrite(value, base));
    if (options.paths) {
      // 相对 paths 的基准取决于最终继承的 baseUrl；不能在当前配置层提前绑定。
      for (const [key, values] of Object.entries(options.paths)) options.paths[key] = values.map(value => {
        if (typeof value !== 'string') throw new Error('paths 映射必须为字符串');
        return path.isAbsolute(value) ? rewrite(value, base) : value;
      });
    }
    for (const key of ['files', 'include', 'exclude']) if (config[key] !== undefined) config[key] = config[key].map(value => rewrite(value, base));
    if (config.extends !== undefined) {
      const resolved = (Array.isArray(config.extends) ? config.extends : [config.extends])
        .map(value => resolveExtends(value, base)).filter(Boolean).map(processConfig);
      if (resolved.length) config.extends = resolved; else delete config.extends;
    }
    if (config.references !== undefined) {
      config.references = config.references.map(reference => {
        const absolute = path.resolve(base, reference.path);
        const file = /\.json$/.test(absolute) ? absolute : path.join(absolute, 'tsconfig.json');
        mapped(context, file, true);
        if (!fs.existsSync(file)) throw new Error(`缺失引用项目配置：${file}`);
        configs.add(file);
        processConfig(file);
        return { ...reference, path: mapped(context, absolute, true) };
      });
    }
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(destination, JSON.stringify(config, null, 2));
    return destination;
  }
  /** 第一方目录递归查找配置，不把 node_modules 中的工具配置注册为项目。 */
  function visit(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      if (entry.isDirectory() && entry.name !== 'node_modules') visit(path.join(directory, entry.name));
      else if (entry.isFile() && /^(?:tsconfig|jsconfig)(?:\.[\w.-]+)?\.json$/.test(entry.name)) {
        configs.add(path.join(context.root, path.relative(context.view, path.join(directory, entry.name))));
      }
    }
  }
  visit(context.view);
  for (const original of configs) processConfig(original);
  const projects = [];
  const candidates = new Map(sources.map(source => [source.view, []]));
  for (const original of configs) {
    const config = mapped(context, original, true);
    const errors = [];
    const parsed = ts.getParsedCommandLineOfConfigFile(config, {}, { ...ts.sys, onUnRecoverableConfigFileDiagnostic: error => errors.push(error) });
    if (!parsed || errors.length) throw new Error(`无法确定项目上下文：${original}`);
    // 继承全部完成后才验证相对映射，pathsBasePath 保留无 baseUrl 时的声明配置目录。
    const effectiveBase = parsed.options.baseUrl ?? parsed.options.pathsBasePath ?? path.dirname(config);
    for (const values of Object.values(parsed.options.paths ?? {})) {
      for (const value of values) {
        const candidate = path.resolve(effectiveBase, value);
        if (!within(context.view, candidate)) throw new Error(`有效 paths 映射越界：${candidate}`);
        let ancestor = candidate;
        while (!fs.existsSync(ancestor) && ancestor !== context.view) ancestor = path.dirname(ancestor);
        if (!within(context.view, fs.realpathSync(ancestor))) throw new Error(`有效 paths 符号链接越界：${candidate}`);
      }
    }
    for (const error of parsed.errors) context.note(original, `TS${error.code}: ${ts.flattenDiagnosticMessageText(error.messageText, '\n')}`);
    for (const file of parsed.fileNames) {
      if (!within(context.view, file)) throw new Error(`配置成员越界：${file}`);
      candidates.get(file)?.push(config);
    }
    projects.push({ name: config, roots: [config], options: {} });
  }
  return { projects, candidates, configFiles: [...done].map(original => mapped(context, original, true)) };
}

/** 未配置文件按最近包元数据选固定 NodeNext 或 Preserve/Bundler；不参考工具 Node 版本。 */
export function inferredOptions(context, file) {
  let directory = path.dirname(file);
  let node = /\.[cm][jt]s$/.test(file);
  while (within(context.view, directory)) {
    const manifest = path.join(directory, 'package.json');
    if (fs.existsSync(manifest)) {
      const metadata = JSON.parse(fs.readFileSync(manifest, 'utf8'));
      node ||= Boolean(metadata.engines?.node || metadata.dependencies?.['@types/node'] || metadata.devDependencies?.['@types/node'] || metadata.type === 'commonjs');
      break;
    }
    if (directory === context.view) break;
    directory = path.dirname(directory);
  }
  // tsserver protocol 使用选项枚举的字符串形式；ES2022 的 lib 名需要完整文件名。
  return { target: 'ES2022', module: node ? 'NodeNext' : 'Preserve', moduleResolution: node ? 'NodeNext' : 'Bundler',
    allowJs: true, checkJs: true, jsx: 'preserve', lib: node ? ['lib.es2022.d.ts'] : ['lib.es2022.d.ts', 'lib.dom.d.ts', 'lib.dom.iterable.d.ts'],
    types: node ? ['node'] : [], typeRoots: [path.join(context.view, 'node_modules/@types')], skipLibCheck: true };
}

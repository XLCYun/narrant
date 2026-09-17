import fs from 'node:fs';
import path from 'node:path';
import { within, checkedWorkspacePath, workspaceSystem } from './workspace.mjs';

/** 固定 TS 仅用于 JSONC 配置解析；不创建 Program、checker 或抽取图。 */
export function prepareConfigurations(context, ts, sources) {
  const host = workspaceSystem(context, ts);
  const done = new Set();
  const configs = new Set();
  const pathOptions = ['baseUrl', 'rootDir', 'outDir', 'declarationDir', 'tsBuildInfoFile'];
  const listPathOptions = ['rootDirs', 'typeRoots'];
  /** 在内存中把配置路径规范到 Git workspace，不存在的输出目录也不能越界。 */
  function rewrite(value, base) {
    if (typeof value !== 'string') throw new Error('配置路径必须为字符串');
    // 原样交给固定 TS 在最终消费配置处展开，不能提前绑定到共享配置的目录。
    if (value.toLowerCase().startsWith('${configdir}')) return value;
    return checkedWorkspacePath(context.root, path.resolve(base, value));
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
      checkedWorkspacePath(context.root, candidate);
      const candidates = [candidate, `${candidate}.json`, path.join(candidate, 'tsconfig.json')];
      const manifest = path.join(candidate, 'package.json');
      if (fs.existsSync(manifest)) {
        checkedWorkspacePath(context.root, manifest);
        const packageData = JSON.parse(fs.readFileSync(manifest, 'utf8'));
        if (typeof packageData.tsconfig === 'string') candidates.unshift(path.resolve(candidate, packageData.tsconfig));
      }
      for (const file of candidates) {
        if (fs.existsSync(file) && fs.statSync(file).isFile()) {
          if (!/\.jsonc?$/.test(file)) throw new Error(`不执行配置代码：${file}`);
          checkedWorkspacePath(context.root, file);
          return fs.realpathSync(file);
        }
      }
    }
    return null;
  }
  /** 保留每层 extends/references；只剥离插件入口和改写路径。 */
  function processConfig(original) {
    original = fs.realpathSync(checkedWorkspacePath(context.root, original));
    if (done.has(original)) return original;
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
        checkedWorkspacePath(context.root, file);
        if (!fs.existsSync(file)) throw new Error(`缺失引用项目配置：${file}`);
        configs.add(fs.realpathSync(file));
        processConfig(file);
        return { ...reference, path: checkedWorkspacePath(context.root, absolute) };
      });
    }
    context.metadata.add(original);
    context.overlays.set(original, JSON.stringify(config));
    return original;
  }
  // 配置来自同一个 workspace 的清单；不扫描生成目录或安装依赖的配置。
  for (const file of context.metadata) {
    if (/^(?:tsconfig|jsconfig)(?:\.[\w.-]+)?\.json$/.test(path.basename(file))) configs.add(file);
  }
  for (const original of configs) processConfig(original);
  const projects = [];
  const candidates = new Map(sources.map(source => [source, []]));
  for (const original of configs) {
    const config = checkedWorkspacePath(context.root, original);
    const errors = [];
    const parsed = ts.getParsedCommandLineOfConfigFile(config, {}, { ...host, onUnRecoverableConfigFileDiagnostic: error => errors.push(error) });
    if (!parsed || errors.length) throw new Error(`无法确定项目上下文：${original}`);
    // 继承与模板展开全部完成后校验有效路径；不存在的输出和 glob 也检查最近现存祖先。
    /** 校验有效路径和现存祖先，工作区外的配置仍然拒绝。 */
    function validateEffectivePath(value, base, label) {
      const candidate = path.resolve(base, value);
      if (!within(context.root, candidate)) throw new Error(`有效 ${label} 映射越界：${candidate}`);
      checkedWorkspacePath(context.root, candidate);
    }
    const configDirectory = path.dirname(config);
    for (const key of pathOptions) {
      if (parsed.options[key] !== undefined) validateEffectivePath(parsed.options[key], configDirectory, key);
    }
    for (const key of listPathOptions) {
      for (const value of parsed.options[key] ?? []) validateEffectivePath(value, configDirectory, key);
    }
    // pathsBasePath 保留无 baseUrl 时的声明配置目录；模板 paths 已由 TS 展开。
    const effectiveBase = parsed.options.baseUrl ?? parsed.options.pathsBasePath ?? configDirectory;
    for (const values of Object.values(parsed.options.paths ?? {})) {
      for (const value of values) validateEffectivePath(value, effectiveBase, 'paths');
    }
    // 固定 TS 6.0.3 的配置解析结果包含展开后的有效文件模式，不自行重写模板语法。
    const specs = parsed.options.configFile.configFileSpecs;
    for (const key of ['validatedFilesSpec', 'validatedIncludeSpecs', 'validatedExcludeSpecs']) {
      for (const value of specs[key] ?? []) validateEffectivePath(value, configDirectory, key);
    }
    for (const file of parsed.fileNames) {
      validateEffectivePath(file, configDirectory, '配置成员');
      candidates.get(file)?.push(config);
    }
    projects.push({ name: config, roots: [config], options: {} });
  }
  return { host, projects, candidates, configFiles: [...done].map(original => checkedWorkspacePath(context.root, original)) };
}

/** 未配置文件按最近包元数据选固定 NodeNext 或 Preserve/Bundler；不参考工具 Node 版本。 */
export function inferredOptions(context, file) {
  let directory = path.dirname(file);
  let node = /\.[cm][jt]s$/.test(file);
  while (within(context.root, directory)) {
    const manifest = path.join(directory, 'package.json');
    if (fs.existsSync(manifest)) {
      const metadata = JSON.parse(fs.readFileSync(manifest, 'utf8'));
      node ||= Boolean(metadata.engines?.node || metadata.dependencies?.['@types/node'] || metadata.devDependencies?.['@types/node'] || metadata.type === 'commonjs');
      break;
    }
    if (directory === context.root) break;
    directory = path.dirname(directory);
  }
  // tsserver protocol 使用选项枚举的字符串形式；ES2022 的 lib 名需要完整文件名。
  return { target: 'ES2022', module: node ? 'NodeNext' : 'Preserve', moduleResolution: node ? 'NodeNext' : 'Bundler',
    allowJs: true, checkJs: true, jsx: 'preserve', lib: node ? ['lib.es2022.d.ts'] : ['lib.es2022.d.ts', 'lib.dom.d.ts', 'lib.dom.iterable.d.ts'],
    types: node ? ['node'] : [], typeRoots: [path.join(context.root, 'node_modules/@types')], skipLibCheck: true };
}

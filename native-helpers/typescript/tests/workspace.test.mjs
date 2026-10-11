import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { prepare, runtime, ts } from './support.mjs';

/** 只在测试夹具创建文件；被测准备流程必须原位读取，不能增加或改写文件。 */
function fixture(t, files) {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'narrant-workspace-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const [name, source] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    fs.writeFileSync(path.join(root, name), source);
  }
  return root;
}

test('configuration overlays and fixed libraries use the unchanged workspace', t => {
  const config = '{"compilerOptions":{"module":"NodeNext","types":["node"],"plugins":[{"name":"business-plugin"}]},"files":["main.ts"]}';
  const source = "import {readFile} from 'node:fs'; export const read = readFile;";
  const root = fixture(t, { 'tsconfig.json': config, 'main.ts': source });
  const { host, output } = prepare({ root, runtime, selected: [path.join(root, 'main.ts')] }, ts);
  assert.equal(host.getCurrentDirectory(), root);
  assert.deepEqual(output.sources, [path.join(root, 'main.ts')]);
  assert.deepEqual(output.config_files, [path.join(root, 'tsconfig.json')]);
  assert.equal(host.readFile(path.join(root, 'main.ts')), source);
  assert.equal(JSON.parse(host.readFile(path.join(root, 'tsconfig.json'))).compilerOptions.plugins, undefined);
  const node = ts.resolveTypeReferenceDirective('node', path.join(root, 'main.ts'), {}, host).resolvedTypeReferenceDirective;
  assert.equal(node.resolvedFileName, ts.normalizePath(path.join(runtime, '@types/node/index.d.ts')));
  const undici = ts.resolveModuleName('undici-types', path.join(runtime, '@types/node/web-globals/fetch.d.ts'),
    { moduleResolution: ts.ModuleResolutionKind.NodeNext }, host).resolvedModule;
  assert.equal(undici.resolvedFileName, ts.normalizePath(path.join(runtime, 'undici-types/index.d.ts')));
  assert.throws(() => host.writeFile(path.join(root, 'main.ts'), 'changed'), /不能写入/);
  assert.throws(() => host.createDirectory(path.join(root, 'generated')), /不能创建/);
  assert.deepEqual(fs.readdirSync(root).sort(), ['main.ts', 'tsconfig.json']);
  assert.equal(fs.readFileSync(path.join(root, 'main.ts'), 'utf8'), source);
  assert.equal(fs.readFileSync(path.join(root, 'tsconfig.json'), 'utf8'), config);
});

test('glob traversal skips Cargo output and business dependencies, preserving ordinary target sources', t => {
  const root = fixture(t, {
    'tsconfig.json': '{"compilerOptions":{"allowJs":true},"include":["**/*"]}',
    'Cargo.toml': '', 'target/deep/package.json': 'invalid generated JSON', 'target/deep/generated.ts': '',
    'src/target/main.ts': 'export const kept = true;',
    'node_modules/business/index.d.ts': 'export const hidden: true;',
    'node_modules/business/package.json': '{"types":"index.d.ts"}',
  });
  const { host, output } = prepare({ root, runtime, selected: [path.join(root, 'src/target/main.ts')] }, ts);
  assert.deepEqual(output.sources, [path.join(root, 'src/target/main.ts')]);
  assert.equal(host.directoryExists(path.join(root, 'target')), false);
  assert.equal(host.directoryExists(path.join(root, 'src/target')), true);
  assert.equal(host.readFile(path.join(root, 'node_modules/business/index.d.ts')), undefined);
  assert.equal(host.fileExists(path.join(root, 'node_modules/business/package.json')), false);
  const parsed = ts.getParsedCommandLineOfConfigFile(path.join(root, 'tsconfig.json'), {}, host);
  assert.deepEqual(parsed.fileNames, output.sources.map(ts.normalizePath));
});

test('workspace package resolution uses original source without creating node_modules links', t => {
  const root = fixture(t, {
    'package.json': '{"workspaces":["packages/*"]}',
    'main.ts': "import {run} from '@sample/lib'; run();",
    'packages/lib/package.json': '{"name":"@sample/lib","exports":{".":"./src/index.ts"}}',
    'packages/lib/src/index.ts': 'export function run() {}',
  });
  const { host } = prepare({ root, runtime, selected: [path.join(root, 'main.ts')] }, ts);
  const target = ts.resolveModuleName('@sample/lib', path.join(root, 'main.ts'),
    { moduleResolution: ts.ModuleResolutionKind.Bundler }, host).resolvedModule;
  assert.equal(target.resolvedFileName, ts.normalizePath(path.join(root, 'packages/lib/src/index.ts')));
  assert.equal(fs.existsSync(path.join(root, 'node_modules')), false);
});

/** 原生短路径或目录别名仍通过同一真实根解析虚拟 workspace 包，不增加授权文件。 */
test('workspace package resolution preserves an existing root alias', t => {
  const root = fixture(t, {
    'package.json': '{"workspaces":["packages/*"]}',
    'main.ts': "import {run} from '@sample/lib'; run();",
    'packages/lib/package.json': '{"name":"@sample/lib","exports":{".":"./src/index.ts"}}',
    'packages/lib/src/index.ts': 'export function run() {}',
  });
  const alias = `${root}-alias`;
  fs.symlinkSync(root, alias, 'junction');
  t.after(() => fs.rmSync(alias, { recursive: true, force: true }));
  const { host } = prepare({ root: alias, runtime, selected: [path.join(alias, 'main.ts')] }, ts);
  const target = ts.resolveModuleName('@sample/lib', path.join(alias, 'main.ts'),
    { moduleResolution: ts.ModuleResolutionKind.Bundler }, host).resolvedModule;
  assert.equal(target?.resolvedFileName?.replaceAll('\\', '/'), path.join(fs.realpathSync.native(root), 'packages/lib/src/index.ts').replaceAll('\\', '/'));
  assert.equal(fs.existsSync(path.join(root, 'node_modules')), false);
});

for (const manifest of [
  { types: '../outside.d.ts' },
  { exports: { '.': { types: '../outside.d.ts' } } },
  { imports: { '#outside': '../outside.ts' } },
]) {
  test(`package paths cannot escape through ${Object.keys(manifest)[0]}`, t => {
    const root = fixture(t, { 'main.ts': '', 'package.json': JSON.stringify(manifest) });
    assert.throws(() => prepare({ root, runtime, selected: [path.join(root, 'main.ts')] }, ts), /仓库外/);
  });
}

test('internal config symlinks still use sanitized in-memory configuration', t => {
  const config = '{"compilerOptions":{"plugins":[{"name":"must-not-run"}],"types":[]}}';
  const root = fixture(t, { 'main.ts': '', 'base.json': config, 'tsconfig.json': '{"extends":"./linked.json","files":["main.ts"]}' });
  fs.symlinkSync('base.json', path.join(root, 'linked.json'));
  const { host } = prepare({ root, runtime, selected: [path.join(root, 'main.ts')] }, ts);
  assert.equal(JSON.parse(host.readFile(path.join(root, 'linked.json'))).compilerOptions.plugins, undefined);
  assert.equal(fs.readFileSync(path.join(root, 'linked.json'), 'utf8'), config);
  assert.equal(fs.lstatSync(path.join(root, 'linked.json')).isSymbolicLink(), true);
});

/** 不同 realpath 实现返回不同大小写时，已有文件仍以工作区清单的真实路径授权。 */
test('workspace authorization uses one realpath identity for a case alias', t => {
  const root = fixture(t, { 'main.ts': 'export const value = 1;' });
  const alias = path.join(path.dirname(root), path.basename(root).toUpperCase());
  if (!fs.existsSync(alias)) return t.skip('测试文件系统区分大小写，不提供同一文件的大小写别名');
  const file = path.join(alias, 'main.ts');
  assert.equal(fs.statSync(file).ino, fs.statSync(path.join(root, 'main.ts')).ino);
  const { host, context } = prepare({ root: alias, runtime, selected: [file] });
  assert.equal(host.fileExists(file), true);
  assert.equal(host.realpath(file), [...context.sources][0]);
  assert.equal(host.readFile(file), 'export const value = 1;');
});

for (const alias of ['MAIN.ts', 'nested/MAIN.ts', 'NESTED/main.ts', 'NESTED/MAIN.ts']) {
  /** 文件名和子目录别名必须回到同一真实清单项，不猜测大小写或添加授权路径。 */
  test(`workspace authorization preserves existing case alias ${alias}`, t => {
    const root = fixture(t, { 'main.ts': 'export const value = 1;', 'nested/main.ts': 'export const nested = 2;' });
    const file = path.join(root, alias);
    if (!fs.existsSync(file)) return t.skip('测试文件系统不提供大小写别名');
    const original = path.join(root, alias.startsWith('MAIN') ? 'main.ts' : 'nested/main.ts');
    assert.equal(fs.statSync(file).ino, fs.statSync(original).ino);
    const { host, context } = prepare({ root, runtime, selected: [original] });
    assert.equal(host.fileExists(file), true);
    assert.equal(host.realpath(file), host.realpath(original));
    assert.equal(context.sources.has(host.realpath(file)), true);
    assert.equal(host.readFile(file), fs.readFileSync(original, 'utf8'));
  });
}

/** 配置别名只读取已经清洗的覆盖层，真实越界链接仍在原生 host 中失败。 */
test('workspace metadata case aliases preserve overlays and external junctions remain rejected', t => {
  const root = fixture(t, { 'main.ts': '', 'tsconfig.json': '{"compilerOptions":{"plugins":[{"name":"must-not-run"}]},"files":["main.ts"]}' });
  const alias = path.join(root, 'TSCONFIG.json');
  if (!fs.existsSync(alias)) return t.skip('测试文件系统不提供大小写别名');
  const { host } = prepare({ root, runtime, selected: [path.join(root, 'main.ts')] });
  assert.equal(host.fileExists(alias), true);
  assert.equal(JSON.parse(host.readFile(alias)).compilerOptions.plugins, undefined);
  const outside = fs.mkdtempSync(path.join(os.tmpdir(), 'narrant-workspace-outside-'));
  t.after(() => fs.rmSync(outside, { recursive: true, force: true }));
  fs.writeFileSync(path.join(outside, 'main.ts'), 'export const escaped = true;');
  fs.symlinkSync(outside, path.join(root, 'escape'), 'junction');
  assert.throws(() => host.fileExists(path.join(root, 'escape/main.ts')), /服务引入未授权源码/);
  assert.throws(() => host.readFile(path.join(root, 'escape/main.ts')), /服务引入未授权源码/);
});

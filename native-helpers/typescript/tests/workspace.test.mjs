import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { prepare, runtime, ts } from './support.mjs';

/** 只在测试夹具创建文件；被测准备流程必须原位读取，不能增加或改写文件。 */
function fixture(t, files) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'narrant-workspace-')));
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
  assert.equal(node.resolvedFileName, path.join(runtime, '@types/node/index.d.ts'));
  const undici = ts.resolveModuleName('undici-types', path.join(runtime, '@types/node/web-globals/fetch.d.ts'),
    { moduleResolution: ts.ModuleResolutionKind.NodeNext }, host).resolvedModule;
  assert.equal(undici.resolvedFileName, path.join(runtime, 'undici-types/index.d.ts'));
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
  assert.deepEqual(parsed.fileNames, output.sources);
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
  assert.equal(target.resolvedFileName, path.join(root, 'packages/lib/src/index.ts'));
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

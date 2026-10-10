import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { createWorkspace } from '../workspace.mjs';
import { prepareConfigurations } from '../config.mjs';

import { runtime, ts } from './support.mjs';

assert.equal(ts.version, '6.0.3');

/** 真实文件系统同时提供源码、竞争目标和被排除成员，不伪造 readDirectory。 */
function fixture(t, inherited, extra = {}) {
  const temporary = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'narrant-config-')));
  t.after(() => fs.rmSync(temporary, { recursive: true, force: true }));
  const root = path.join(temporary, 'repo');
  const config = {
    compilerOptions: {
      module: 'Preserve', moduleResolution: 'Bundler', types: [],
      baseUrl: '${configDir}', rootDir: '${configDir}/src', outDir: '${configDir}/dist',
      declarationDir: '${configDir}/declarations', tsBuildInfoFile: '${configDir}/cache/build.tsbuildinfo',
      rootDirs: ['${configDir}/src', '${configDir}/generated'], typeRoots: ['${configDir}/types'],
      paths: { '@app/*': ['src/*'], '@explicit/*': ['${configDir}/src/*'] },
      ...extra,
    },
    files: ['${configDir}/extra.ts'],
    include: ['${configDir}/src/**/*.ts'], exclude: ['${configDir}/src/excluded/**'],
  };
  const files = {
    'app/tsconfig.json': JSON.stringify(inherited ? { extends: '../config/base.json' } : config),
    ...(inherited ? { 'config/base.json': JSON.stringify(config) } : {}),
    'app/src/main.ts': "import {target} from '@app/dep'; export function caller() { target(); }",
    'app/src/dep.ts': 'export function target() { return "correct consumer"; }',
    'app/src/excluded/hidden.ts': 'export const hidden = true;',
    'app/extra.ts': 'export const extra = true;',
    'config/src/dep.ts': 'export function target() { return "wrong shared base"; }',
    'src/dep.ts': 'export function target() { return "wrong repository base"; }',
  };
  for (const [name, source] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    fs.writeFileSync(path.join(root, name), source);
  }
  return createWorkspace({ root, runtime });
}

/** 固定编译器负责模板展开、成员匹配与别名解析；这里只归一化临时根供比较。 */
function snapshot(root, host = ts.sys) {
  const parsed = ts.getParsedCommandLineOfConfigFile(path.join(root, 'app/tsconfig.json'), {}, {
    ...host, onUnRecoverableConfigFileDiagnostic(error) { throw new Error(String(error.code)); },
  });
  const options = Object.fromEntries(['baseUrl', 'rootDir', 'outDir', 'declarationDir', 'tsBuildInfoFile', 'rootDirs', 'typeRoots', 'paths']
    .map(key => [key, parsed.options[key]]));
  const targets = ['@app/dep', '@explicit/dep'].map(name => ts.resolveModuleName(name,
    path.join(root, 'app/src/main.ts'), parsed.options, host).resolvedModule?.resolvedFileName);
  return JSON.parse(JSON.stringify({ options, files: parsed.fileNames.sort(), targets }).replaceAll(root, '<root>'));
}

for (const inherited of [false, true]) {
  test(`configDir ${inherited ? 'inherited' : 'direct'} preserves native options, membership and aliases`, t => {
    const context = fixture(t, inherited);
    const original = snapshot(context.root);
    assert.deepEqual(original.files, ['<root>/app/extra.ts', '<root>/app/src/dep.ts', '<root>/app/src/main.ts']);
    assert.deepEqual(original.targets, ['<root>/app/src/dep.ts', '<root>/app/src/dep.ts']);
    const sources = [...context.sources];
    const prepared = prepareConfigurations(context, ts, sources);
    assert.deepEqual(snapshot(context.root, prepared.host), original);
    assert.deepEqual(prepared.candidates.get(path.join(context.root, 'app/src/main.ts')),
      [path.join(context.root, 'app/tsconfig.json')]);
    assert.deepEqual(prepared.candidates.get(path.join(context.root, 'app/src/excluded/hidden.ts')), []);
  });
}

/** 模板展开后仍需验证未存在输出、模式和链接祖先，不能只检查实际命中的文件。 */
for (const [key, value] of Object.entries({
  baseUrl: '${configDir}/../../outside', rootDir: '${configDir}/../../outside',
  outDir: '${configDir}/../../outside', declarationDir: '${configDir}/../../outside',
  tsBuildInfoFile: '${configDir}/../../outside/cache',
  rootDirs: ['${configDir}/../../outside'], typeRoots: ['${configDir}/../../outside'],
  paths: { '@app/*': ['${configDir}/../../outside/*'] },
})) {
  test(`configDir rejects effective ${key} escape`, t => {
    const context = fixture(t, true, { [key]: value });
    const sources = [...context.sources];
    assert.throws(() => prepareConfigurations(context, ts, sources), /越界|仓库外/);
  });
}
for (const field of ['files', 'include', 'exclude']) {
  test(`configDir rejects effective ${field} escape with no matching files`, t => {
    const context = fixture(t, true);
    const file = path.join(context.root, 'config/base.json');
    const config = JSON.parse(fs.readFileSync(file, 'utf8'));
    config[field] = ['${configDir}/../../outside/missing.ts'];
    fs.writeFileSync(file, JSON.stringify(config));
    const sources = [...context.sources];
    assert.throws(() => prepareConfigurations(context, ts, sources), /越界|仓库外/);
  });
}
test('configDir rejects original symlink ancestor for nonexistent output', t => {
  const context = fixture(t, false, { outDir: '${configDir}/linked/missing' });
  fs.symlinkSync(path.dirname(context.root), path.join(context.root, 'app/linked'), 'dir');
  const sources = [...context.sources];
  assert.throws(() => prepareConfigurations(context, ts, sources), /越界|仓库外/);
});

import fs from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { here } from './support.mjs';

const read = async name => JSON.parse(await fs.readFile(path.join(here, `${name}-results.json`), 'utf8'));
const scope = await read('scope'), modules = await read('module'), projects = await read('project');
const checks = [];

/** 用源码归属与声明范围验证结果，不把任意非空定义误认为成功。 */
function check(name, fn) { fn(); checks.push(name); }
function targets(defs, dir) {
  return (defs ?? []).map(d => path.relative(dir, fileURLToPath(d.targetUri ?? d.uri)));
}
const files = project => project.body.fileNames;
const isDependency = file => file.includes('/node_modules/probe-third-party/');

for (const c of [...scope.cases, ...modules.cases, ...projects.cases]) check(`${c.engine ?? 'ts6'} ${c.name ?? c.mode}: 正常关闭`, () => {
  assert.equal(c.error, undefined); assert.equal(c.lifecycle.shutdownError, undefined);
  assert.equal(c.lifecycle.exit.code, 0); assert.deepEqual(c.lifecycle.remaining, []);
});
for (const c of scope.cases) check(`${c.engine} ${c.mode}: 范围与本地调用`, () => {
  assert.equal(c.project.body.configFileName, path.join(c.dir, 'narrant.external'));
  assert.deepEqual(targets(c.localTs, c.dir), ['model.ts']);
  assert.deepEqual(targets(c.localJs, c.dir), ['model.ts']);
  assert.equal(files(c.project).some(isDependency), c.mode === 'normal');
  assert.equal(c.thirdParty.length > 0, c.mode === 'normal');
  if (c.beforeProject) assert.equal(files(c.beforeProject).some(isDependency), true);
});
for (const c of modules.cases.filter(c => c.name.startsWith('alias-'))) check(`${c.name}: baseUrl + paths + barrel`, () => {
  assert.deepEqual(targets(c.queries[0].response, c.dir), ['src/model.ts']);
});
for (const c of modules.cases.filter(c => c.name === 'node-nodenext' || c.name === 'node-node16')) check(`${c.name}: 条件导出与封闭子路径`, () => {
  const expected = ['esm.ts', 'esm.ts', 'relative.ts', 'allowed.ts', 'entry.mts', 'cjs.cts', 'cjs.cts', 'esm.ts', 'cjs.cts'];
  c.queries.forEach((q, i) => assert.deepEqual(targets(q.response, c.dir), [expected[i]]));
  assert.equal(c.queries[4].response[0].targetSelectionRange.start.line, 4); // 指回 import，而非 forbidden 的声明。
});
const jsx = modules.cases.find(c => c.name === 'jsx-jsdoc');
check('JSX / inline callback / 原生 JSDoc', () => {
  assert.deepEqual(jsx.queries[1].response.outgoing[0].map(c => c.to.name), ['Card']);
  assert(jsx.queries[2].response.outgoing[0].some(c => c.to.name === 'save'));
  assert.deepEqual(targets(jsx.queries[3].response, jsx.dir), ['model.ts']);
  assert.equal(jsx.queries[4].response[0].targetSelectionRange.start.line, 3);
});
check('受控 Node 声明恢复本地 Service.save', () => {
  const without = modules.cases.find(c => c.name === 'node-runtime-false');
  const withNode = modules.cases.find(c => c.name === 'node-runtime-true');
  assert.deepEqual(without.queries[0].response, []);
  assert.deepEqual(targets(withNode.queries[0].response, withNode.dir), ['main.ts']);
  assert.equal(withNode.queries[0].response[0].targetSelectionRange.start.line, 2);
  assert(withNode.queries[1].response[0].targetUri.endsWith('/fs/promises.d.ts'));
  assert(withNode.queries[2].response[0].targetUri.endsWith('/buffer.d.ts'));
});
for (const c of projects.cases.filter(c => c.name.startsWith('references-'))) check(`${c.name}: references 与第三方范围`, () => {
  const resolves = c.name === 'references-configured' || c.name.includes('-view-');
  assert.deepEqual(targets(c.local, c.dir), resolves ? ['lib/src/index.ts'] : []);
  assert.equal(files(c.projects[0]).some(isDependency), c.name === 'references-configured');
  assert.equal(c.thirdParty.length > 0, c.name === 'references-configured');
});
for (const c of projects.cases.filter(c => c.name.startsWith('shared-'))) check(`${c.name}: 显式项目上下文`, () => {
  const names = c.name.slice('shared-'.length).split('-');
  assert.deepEqual(targets(c.standardDefinition, c.dir), [`${names[0]}/impl.ts`]);
  c.explicitDefinitions.forEach(d => assert.deepEqual(d.response.body.map(x => path.relative(c.dir, x.file)), [`${d.project}/impl.ts`]));
});
const summary = { date: '2026-09-14', sessions: scope.cases.length + modules.cases.length + projects.cases.length, checks: checks.length, passed: checks };
await fs.writeFile(path.join(here, 'checked-results.json'), JSON.stringify(summary, null, 2) + '\n');
console.log(JSON.stringify(summary, null, 2));

import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { analyze } from '../analyze.mjs';
import { ts, runtime } from './support.mjs';

/** 每个案例在独立真实目录中提供输入，生产读取政策必须生效。 */
function fixture(t, files) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'native-ts-test-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const [name, content] of Object.entries(files)) { fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true }); fs.writeFileSync(path.join(root, name), content); }
  return root;
}
/** 协议平台目录来自实际包，同调用生产 helper 的参数一致。 */
function request(root, files, include_tests = false) {
  return { input: { repository_root: root, files, include_tests, profile: 'typescript-static-v1' }, environment: { platforms: [['typescript','6.0.3'], ['@types/node','22.20.1'], ['undici-types','6.21.0']].map(([name, version]) => ({ id: name, kind: 'typescript-declarations', version, root: path.join(runtime, name, name === 'typescript' ? 'lib' : ''), metadata: { package: name } })) } };
}
test('fixed_service_excludes_business_and_supplies_node_closure', t => {
  const root = fixture(t, {
    'tsconfig.json': '{"compilerOptions":{"module":"NodeNext","types":["node"],"lib":["ES2022"]},"files":["main.ts"]}',
    'main.ts': "import {business} from 'business'; import {readFile} from 'node:fs/promises'; export async function run() { business(); await readFile('x'); Promise.resolve(1); }",
    'node_modules/business/package.json': '{"types":"index.d.ts"}',
    'node_modules/business/index.d.ts': 'export function business(): void;',
  });
  const { graph, evidence } = analyze(ts, request(root,['main.ts']));
  const refs = graph.nodes.find(n => n.name === 'run').references;
  assert.ok(refs.some(r => r.target.kind === 'unresolved' && r.target.name === 'business'));
  for (const name of ['readFile', 'resolve']) assert.ok(graph.external_symbols.some(s => s.name === name && s.scope === 'dependency'));
  assert.deepEqual(evidence.files.map(f => f.path), ['main.ts']);
  assert.equal(evidence.origins.length, graph.nodes.length);
  assert.ok(evidence.origins.every(o => o.selected_paths.length === 1 && o.selected_paths[0] === 'main.ts'));
});
test('fixed_service_rejects_external_source_and_configuration_paths', t => {
  const outer = fixture(t, { 'outside.ts':'export function outside() {}', 'config.json':'{}', 'repo/main.ts': "import {outside} from '../outside'; export function run(){outside()}" });
  const root=path.join(outer,'repo');
  assert.throws(()=>analyze(ts,request(root,['main.ts'])), /未授权源码|仓库外/);
  fs.writeFileSync(path.join(root,'main.ts'),'export function run(){}');
  fs.writeFileSync(path.join(root,'tsconfig.json'),'{"extends":"../config.json"}');
  assert.throws(()=>analyze(ts,request(root,['main.ts'])), /仓库外/);
});
test('invalid syntax and early errors cannot become empty successful graphs', t => {
  const root=fixture(t,{'main.ts':''});
  for(const source of ['function {','const x;','let x; let x;','function f(){break;}','const regex = /[/;',
    'return 1;', 'function f(){"use strict"; with(x){}}', 'function f(a,a){"use strict"; return a}',
    '// @ts-nocheck\nreturn 1;', '// @ts-ignore\nreturn 1;', 'function f(){await x;}',
    'function f(eval){"use strict";return eval}', 'function f(a=1){"use strict"}',
    'function f(){const [a];}', 'function f(...a,b){}', 'const o={get x(a){return a}}',
    'function f(){a?.x=1}', 'class C{constructor(){} constructor(){}}',
    'class C{static{return 1}}', 'class C{#x=1;m(){delete this.#x}}',
    'class C{m(){return this.#missing}}',
  ]) {
    fs.writeFileSync(path.join(root,'main.ts'),source);
    assert.throws(()=>analyze(ts,request(root,['main.ts'])),/无法解析/,source);
  }
});
test('selected coverage and external descriptors preserve test policy', t => {
  const root=fixture(t,{'main.ts':"import {testHelper} from './hidden.test'; export function run(){testHelper()}",'hidden.test.ts':'export function testHelper() {}','routes.ts':"export * from './main'"});
  const noTests=analyze(ts,request(root,['main.ts','hidden.test.ts','routes.ts']));
  assert.deepEqual(noTests.evidence.files.map(f=>f.disposition),['analyzed','excluded_test','analyzed']);
  assert.ok(noTests.graph.external_symbols.some(s=>s.name==='testHelper' && s.scope==='repository'));
  const yesTests=analyze(ts,request(root,['main.ts','hidden.test.ts'],true));
  assert.ok(yesTests.graph.nodes.find(n=>n.name==='run').references.some(r=>r.target.kind==='node'));
});
test('JSX intrinsic names and tagged template calls retain their intents', t => {
  const root=fixture(t,{'main.tsx':'function tag(x) {} function run() { tag`text`; return <><X-widget/><button/><this/><UI.Card/><this.Card/><_Card/><组件/></>; }'});
  const graph=analyze(ts,request(root,['main.tsx'])).graph;
  const refs=graph.nodes.find(n=>n.name==='run').references;
  assert.deepEqual(refs.filter(r=>r.semantic==='jsx.uses_component').map(r=>r.target.name).sort(),['Card','_Card','组件'].sort());
  assert.ok(refs.some(r=>r.semantic==='code.calls' && r.target.kind==='node'));
});

test('original BOM and UTF-16 positions survive, invalid selected UTF-8 fails', t => {
  const source='\uFEFFfunction 调用(){}\r\nexport function caller(){"😀";调用();}';
  const root=fixture(t,{'main.ts':source});
  const graph=analyze(ts,request(root,['main.ts'])).graph;
  assert.equal(graph.nodes.find(n=>n.native_kind==='file').source,source);
  assert.ok(graph.nodes.find(n=>n.name==='caller').references.some(r=>r.target.id?.endsWith('::function:%E8%B0%83%E7%94%A8')));
  fs.writeFileSync(path.join(root,'main.ts'),Buffer.from([255]));
  assert.throws(()=>analyze(ts,request(root,['main.ts'])));
});

test('jsconfig, inherited paths and package imports/exports use original first-party sources', t => {
  for (const [label, files, selected] of [
    ['jsconfig', {'jsconfig.json':'{"compilerOptions":{"module":"Preserve","moduleResolution":"Bundler","target":"ES2022","baseUrl":".","paths":{"@code/*":["src/target/*"]},"types":[]},"include":["src"]}', 'src/main.js':"import {target} from '@code/worker'; export function caller(){target()}", 'src/target/worker.js':'export function target(){}'}, ['src/main.js','src/target/worker.js']],
    ['inherited baseUrl', {'base.json':'{"compilerOptions":{"baseUrl":".","module":"Preserve","moduleResolution":"Bundler","types":[]}}','app/tsconfig.json':'{"extends":"../base.json","compilerOptions":{"paths":{"@impl":["lib.ts"]}},"files":["main.ts"]}','app/main.ts':"import {target} from '@impl'; export function caller(){target()}",'lib.ts':'export function target(){}','app/lib.ts':'export function wrong(){}'}, ['app/main.ts','lib.ts']],
    ['inherited paths', {'base.json':'{"compilerOptions":{"baseUrl":".","paths":{"@impl":["lib.ts"]},"module":"Preserve","moduleResolution":"Bundler","types":[]}}','app/tsconfig.json':'{"extends":"../base.json","compilerOptions":{"baseUrl":"."},"files":["main.ts"]}','app/main.ts':"import {target} from '@impl'; export function caller(){target()}",'lib.ts':'export function wrong(){}','app/lib.ts':'export function target(){}'}, ['app/main.ts','app/lib.ts']],
    ['package imports', {'package.json':'{"type":"module","imports":{"#impl":"./target.ts"}}','main.mts':"import {target} from '#impl'; export function caller(){target()}",'target.ts':'export function target(){}'}, ['main.mts','target.ts']],
    ['business config metadata', {'node_modules/@config/base/package.json':'{"tsconfig":"base.json"}','node_modules/@config/base/base.json':'{"compilerOptions":{"types":[],"plugins":[{"name":"must-not-execute"}]}}','tsconfig.json':'{"extends":"@config/base","files":["main.ts","target.ts"]}','main.ts':"import {target} from './target'; export function caller(){target()}",'target.ts':'export function target(){}'}, ['main.ts','target.ts']],
  ]) {
    const root=fixture(t,files), graph=analyze(ts,request(root,selected)).graph;
    const target=graph.nodes.find(n=>n.name==='target');
    assert.ok(graph.nodes.find(n=>n.name==='caller').references.some(r=>r.target.id===target.id),label);
    for(const [name,source] of Object.entries(files)) assert.equal(fs.readFileSync(path.join(root,name),'utf8'),source);
  }
});
test('missing extends and explicit noLib preserve authoritative project policy', t => {
  const root=fixture(t,{'tsconfig.json':'{"extends":"@business/tsconfig","compilerOptions":{"noLib":true,"types":[]},"files":["main.ts"]}','main.ts':'function work(){} export function caller(){work();Promise.resolve(1)}'});
  const graph=analyze(ts,request(root,['main.ts'])).graph;
  assert.ok(graph.nodes.find(n=>n.name==='caller').references.some(r=>r.target.kind==='node'));
  assert.equal(graph.external_symbols.length,0);
});
test('unconfigured browser and Node defaults keep separate standard environments', t => {
  const browser=fixture(t,{'main.ts':'export function run(){document.createElement("div");Promise.resolve(1)}'});
  const node=fixture(t,{'package.json':'{"type":"commonjs"}','main.cts':'export function run(){document.createElement("div");Promise.resolve(1)}'});
  const a=analyze(ts,request(browser,['main.ts'])).graph, b=analyze(ts,request(node,['main.cts'])).graph;
  assert.ok(a.external_symbols.some(s=>s.name==='createElement'));
  assert.ok(!b.external_symbols.some(s=>s.name==='createElement'));
  for(const graph of [a,b]) assert.ok(graph.external_symbols.some(s=>s.name==='resolve'));
});

test('cross-file semantic conflicts are not promoted to local grammar failures', t => {
  const root=fixture(t,{'tsconfig.json':'{"files":["one.ts","two.ts"],"compilerOptions":{"types":[]}}','one.ts':'let shared; function one(){}','two.ts':'let shared; function two(){}'});
  const graph=analyze(ts,request(root,['one.ts','two.ts'])).graph;
  assert.ok(graph.nodes.some(n=>n.name==='one')); assert.ok(graph.nodes.some(n=>n.name==='two'));
});


test('selected aliases preserve lexical and canonical test exclusions', t => {
  const root = fixture(t, { 'target.ts': 'export function target() {}', 'hidden.test.ts': 'export function hidden() {}' });
  try {
    fs.symlinkSync(path.join(root, 'target.ts'), path.join(root, 'alias.test.ts'));
    fs.symlinkSync(path.join(root, 'hidden.test.ts'), path.join(root, 'alias.ts'));
  } catch (error) {
    if (process.platform === 'win32' && error.code === 'EPERM') return t.skip('Windows 未授予文件 symlink 权限');
    throw error;
  }
  const excluded = analyze(ts, request(root, ['alias.test.ts']));
  assert.deepEqual(excluded.graph.nodes, []);
  assert.equal(excluded.evidence.files[0].disposition, 'excluded_test');
  const hidden = analyze(ts, request(root, ['alias.ts']));
  assert.deepEqual(hidden.graph.nodes, []);
  assert.equal(hidden.evidence.files[0].disposition, 'excluded_test');
  const included = analyze(ts, request(root, ['alias.ts'], true));
  assert.ok(included.graph.nodes.find(n => n.name === 'hidden'));
  assert.ok(included.graph.nodes.every(n => n.id.includes('hidden.test.ts')));
  assert.ok(included.evidence.origins.every(o => o.selected_paths[0] === 'alias.ts'));
});

test('physical aliases merge canonical nodes and aggregate only included origins', t => {
  const root = fixture(t, { 'impl/main.ts': 'export function run() { return 1; }', 'impl/hidden.test.ts': 'export function testOnly() {}' });
  for (const alias of ['alias', '__tests__']) fs.symlinkSync(path.join(root, 'impl'), path.join(root, alias), 'junction');
  const files = ['impl/main.ts', 'alias/main.ts', '__tests__/main.ts', 'alias/hidden.test.ts'];
  const without = analyze(ts, request(root, files));
  assert.equal(without.graph.nodes.length, 2);
  assert.equal(without.graph.root_contains.length, 1);
  assert.deepEqual(without.evidence.files.map(f => f.disposition), ['analyzed', 'analyzed', 'excluded_test', 'excluded_test']);
  assert.ok(without.evidence.origins.every(o => JSON.stringify(o.selected_paths) === JSON.stringify(files.slice(0, 2))));
  const withTests = analyze(ts, request(root, files, true));
  assert.equal(withTests.graph.nodes.length, 4);
  assert.ok(withTests.evidence.files.every(f => f.disposition === 'analyzed'));
  const run = withTests.graph.nodes.find(n => n.name === 'run');
  assert.deepEqual(withTests.evidence.origins.find(o => o.node_id === run.id).selected_paths, files.slice(0, 3));
  const outside = fixture(t, { 'escape.ts': 'export function outside() {}' });
  fs.symlinkSync(outside, path.join(root, 'outside'), 'junction');
  assert.throws(() => analyze(ts, request(root, ['outside/escape.ts'])), /越界/);
});

test('empty and fully excluded lexical selection do not load unrelated invalid configuration', t => {
  const root = fixture(t, { 'tsconfig.json': '{ invalid config', 'a.test.ts': 'invalid syntax {' });
  for (const files of [[], ['a.test.ts']]) {
    const result = analyze(ts, request(root, files));
    assert.deepEqual(result.graph.nodes, []);
    assert.deepEqual(result.evidence.files.map(f => f.disposition), files.map(() => 'excluded_test'));
    assert.ok(result.evidence.configurations.every(c => c.source_paths.length === 0 && c.platform_ids.length === 0));
  }
  assert.throws(() => analyze(ts, request(root, ['a.test.ts'], true)), /无效配置/);
  const outside = fixture(t, { 'a.test.ts': 'export function outside() {}' });
  fs.symlinkSync(outside, path.join(root, '__tests__'), 'junction');
  assert.throws(() => analyze(ts, request(root, ['__tests__/a.test.ts'])), /越界/);
});


test('redirected no-dist context still rejects suppressed early errors without replacing its native snapshot', t => {
  const root = fixture(t, {
    'app/tsconfig.json': '{"compilerOptions":{"composite":true,"noCheck":true},"files":["main.ts"],"references":[{"path":"../lib"}]}',
    'app/main.ts': "import {target} from '../lib/src/main'; export function caller(){return target()}",
    'lib/tsconfig.json': '{"compilerOptions":{"composite":true,"outDir":"dist","rootDir":"src"},"include":["src"]}',
    'lib/src/main.ts': '// @ts-nocheck\nreturn 1; export function target(){return 1}',
  });
  assert.throws(() => analyze(ts, request(root, ['app/main.ts'])), /无法解析/);
  fs.writeFileSync(path.join(root, 'lib/src/main.ts'), '// @ts-nocheck\nexport function target(){missing(); return 1}');
  const { graph } = analyze(ts, request(root, ['app/main.ts']));
  const target = graph.external_symbols.find(s => s.name === 'target');
  assert.ok(target);
  assert.ok(graph.nodes.find(n => n.name === 'caller').references.some(r => r.target.id === target.id));
});

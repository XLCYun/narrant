import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { LspProbe, here, writeFiles, position, uri } from './support.mjs';

const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-tls-modules-')));
const base = { target: 'es2022', lib: ['lib.es2022.d.ts'], allowJs: true, checkJs: false, types: [], noEmit: true, noResolve: true, disableReferencedProjectLoad: true, disableSolutionSearching: true };
const cases = [];

/** 每个实验都经过 LSP 初始化、受控项目、didOpen、标准定义查询与退出。 */
async function run(name, files, options, queries, extraRoots = [], engine = 'ts6') {
  const dir = path.join(root, name);
  await writeFiles(dir, files);
  const server = new LspProbe(dir, engine === 'ts6' ? path.join(here, 'ts6/lib/tsserver.js') : path.resolve('node_modules/typescript/lib/tsserver.js'));
  const result = { name, dir, engine, options: { ...base, ...options } };
  try {
    await server.initialize();
    const rootFiles = Object.keys(files).filter(f => /\.(?:[cm]?[jt]s|[jt]sx)$/.test(f) && !f.startsWith('node_modules/')).map(f => path.join(dir, f));
    result.projectName = path.join(dir, 'narrant.external');
    await server.custom('openExternalProject', { projectFileName: result.projectName, rootFiles: [...rootFiles, ...extraRoots].map(fileName => ({ fileName })), options: result.options, typeAcquisition: { enable: false } });
    const opened = new Set();
    for (const query of queries) {
      if (opened.has(query.file)) continue;
      opened.add(query.file); await server.open(path.join(dir, query.file));
    }
    result.projects = {};
    for (const file of opened) result.projects[file] = await server.custom('projectInfo', { file: path.join(dir, file), needFileNameList: true });
    result.queries = [];
    for (const query of queries) {
      const file = path.join(dir, query.file);
      const response = query.kind === 'hierarchy' ? await server.hierarchy(file, query.marker)
        : query.kind === 'hover' ? await server.request('textDocument/hover', { textDocument: { uri: uri(file) }, position: position(await fs.readFile(file, 'utf8'), query.marker, query.offset ?? 0) })
        : await server.definition(file, query.marker, query.offset ?? 0);
      result.queries.push({ ...query, response });
    }
    result.optionDiagnostics = await server.custom('compilerOptionsDiagnostics-full', { projectFileName: result.projectName });
  } catch (error) { result.error = error.stack; }
  finally { result.lifecycle = await server.stop(); }
  cases.push(result);
  await fs.writeFile(path.join(here, 'module-results.json'), JSON.stringify({ root, cases }, null, 2) + '\n');
  console.log(JSON.stringify({ name, engine, error: result.error, queryResults: result.queries?.map(q => ({ file: q.file, marker: q.marker, result: q.kind ? q.response : q.response?.map(d => d.targetUri ?? d.uri) })), optionDiagnostics: result.optionDiagnostics, exit: result.lifecycle.exit, remaining: result.lifecycle.remaining }));
}

for (const engine of ['ts5', 'ts6']) {
  const name = `alias-${engine}`;
  const dir = path.join(root, name);
  await run(name, {
    'src/model.ts': 'export class Service { save() { return "local"; } }\n',
    'src/barrel.ts': 'export { Service as LocalService } from "./model";\n',
    'src/main.ts': 'import { LocalService } from "@/barrel";\nexport async function aliasCall() { const value = await Promise.resolve(new LocalService()); return value.save(); }\n',
    'tsconfig.json': { compilerOptions: { baseUrl: '.', paths: { '@/*': ['src/*'] }, module: 'ESNext', moduleResolution: 'Bundler' }, include: ['src'] },
  }, { module: 'esnext', moduleResolution: 'bundler', baseUrl: dir, paths: { '@/*': [path.join(dir, 'src/*')] } }, [ { file: 'src/main.ts', marker: 'value.save', offset: 6 } ], [], engine);
}

for (const mode of ['nodenext', 'node16']) {
  await run(`node-${mode}`, {
    'package.json': { name: 'node-probe', type: 'module', imports: { '#service': { import: './esm.ts', require: './cjs.cts' } }, exports: { '.': { import: './esm.ts', require: './cjs.cts' }, './allowed': './allowed.ts' } },
    'esm.ts': 'export class ESMService { save() { return "esm"; } }\n',
    'cjs.cts': 'export class CJSService { save() { return "cjs"; } }\n',
    'relative.ts': 'export class Relative { save() { return "relative"; } }\n',
    'allowed.ts': 'export function allowed() { return true; }\n',
    'private.ts': 'export function forbidden() { return false; }\n',
    'entry.mts': 'import { ESMService } from "#service";\nimport { ESMService as Self } from "node-probe";\nimport { Relative } from "./relative.js";\nimport { allowed } from "node-probe/allowed";\nimport { forbidden } from "node-probe/private";\nexport function fromImport() { return new ESMService().save(); }\nexport function fromSelf() { return new Self().save(); }\nexport function fromRelative() { return new Relative().save(); }\nexport function fromAllowed() { return allowed(); }\nexport function fromPrivate() { return forbidden(); }\n',
    'entry.cts': 'import Service = require("#service");\nimport Self = require("node-probe");\nexport function fromRequire() { return new Service.CJSService().save(); }\nexport function fromSelf() { return new Self.CJSService().save(); }\n',
    'entry.mjs': 'import { ESMService } from "#service";\nexport function fromMjs() { return new ESMService().save(); }\n',
    'entry.cjs': 'const { CJSService } = require("#service");\nfunction fromCjs() { return new CJSService().save(); }\nmodule.exports = { fromCjs };\n',
    'tsconfig.json': { compilerOptions: { module: mode, moduleResolution: mode, allowJs: true }, include: ['*.*'] },
  }, { module: mode, moduleResolution: mode, resolvePackageJsonImports: true, resolvePackageJsonExports: true }, [
    { file: 'entry.mts', marker: 'ESMService().save', offset: 13 },
    { file: 'entry.mts', marker: 'Self().save', offset: 7 },
    { file: 'entry.mts', marker: 'Relative().save', offset: 11 },
    { file: 'entry.mts', marker: 'return allowed()', offset: 7 },
    { file: 'entry.mts', marker: 'return forbidden()', offset: 7 },
    { file: 'entry.cts', marker: 'Service.CJSService().save', offset: 21 },
    { file: 'entry.cts', marker: 'Self.CJSService().save', offset: 18 },
    { file: 'entry.mjs', marker: 'ESMService().save', offset: 13 },
    { file: 'entry.cjs', marker: 'CJSService().save', offset: 13 },
  ]);
}

await run('jsx-jsdoc', {
  'model.ts': 'export class Service { save() { return "ok"; } }\n',
  'jsx.d.ts': 'declare namespace JSX { interface IntrinsicElements { div: any; button: any; } }\n',
  'view.tsx': 'import { Service } from "./model";\nexport const Card = () => <div />;\nexport function Page() { return <div><Card /><button /></div>; }\nexport const chain = (items: Service[]) => items.map(item => item.save());\n',
  'view.js': 'import { Service } from "./model";\n/** @param {import("./model").Service} item */\nexport function withDocs(item) { return item.save(); }\nexport const JsCard = () => <div />;\nexport function JsPage() { return <JsCard />; }\n',
}, { module: 'esnext', moduleResolution: 'bundler', lib: ['lib.es2022.d.ts', 'lib.dom.d.ts'], jsx: 'preserve' }, [
  { file: 'view.tsx', marker: '<Card', offset: 1 },
  { file: 'view.tsx', marker: 'Page', kind: 'hierarchy' },
  { file: 'view.tsx', marker: 'chain', kind: 'hierarchy' },
  { file: 'view.js', marker: 'item.save', offset: 5 },
  { file: 'view.js', marker: '<JsCard', offset: 1 },
]);

const nodeDir = await fs.realpath('node_modules/@types/node');
const undiciDir = await fs.realpath(path.resolve(nodeDir, '../../undici-types'));
/** 明确固定 Node 22 类型集，排除为旧编译器准备的替代声明目录。 */
async function declarations(dir) {
  const result = [];
  for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name.startsWith('ts5.')) continue;
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) result.push(...await declarations(file));
    else if (entry.name.endsWith('.d.ts')) result.push(file);
  }
  return result;
}
const nodeFiles = [...await declarations(nodeDir), ...await declarations(undiciDir)];
for (const enabled of [false, true]) {
  await run(`node-runtime-${enabled}`, {
    'main.ts': 'import { AsyncLocalStorage } from "node:async_hooks";\nimport { readFile } from "node:fs/promises";\nexport class Service { save() { return "ok"; } }\nconst storage = new AsyncLocalStorage<Service>();\nexport function current() { return storage.getStore()?.save(); }\nexport async function read() { const content = await readFile("file.txt"); return content.toString(); }\n',
  }, { module: 'nodenext', moduleResolution: 'nodenext', skipLibCheck: true }, [
    { file: 'main.ts', marker: 'getStore()?.save', offset: 12 },
    { file: 'main.ts', marker: 'await readFile', offset: 6 },
    { file: 'main.ts', marker: 'content.toString', offset: 8 },
  ], enabled ? nodeFiles : []);
}

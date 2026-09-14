import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { LspProbe, here, writeFiles, position } from './support.mjs';

const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-tls-projects-')));
const cases = [];
const base = { target: 'es2022', module: 'nodenext', moduleResolution: 'nodenext', lib: ['lib.es2022.d.ts'], types: [], noEmit: true };

/** 保留每个案例结果，即使某个能力缺失也不掩盖其它实验。 */
async function record(name, body) {
  const dir = path.join(root, name);
  await fs.mkdir(dir, { recursive: true });
  const server = new LspProbe(dir, path.join(here, 'ts6/lib/tsserver.js'));
  const result = { name, dir };
  try { await server.initialize(); await body(server, result, dir); }
  catch (error) { result.error = error.stack; }
  finally { result.lifecycle = await server.stop(); }
  cases.push(result);
  await fs.writeFile(path.join(here, 'project-results.json'), JSON.stringify({ root, cases }, null, 2) + '\n');
  console.log(JSON.stringify({ ...result, projects: result.projects?.map(project => ({ ...project, body: { ...project.body, fileNames: project.body?.fileNames?.filter(f => !f.includes('/lib/lib.')) } })), lifecycle: { exit: result.lifecycle.exit, remaining: result.lifecycle.remaining } }));
}

const referenceModes = process.env.PROBE_REFERENCE_MODES?.split(',') ?? ['configured', 'flat-external', 'controlled-configured', 'view-paths', 'view-workspace'];
for (const mode of referenceModes) {
  await record(`references-${mode}`, async (server, result, dir) => {
    const controlled = mode === 'controlled-configured';
    const view = mode.startsWith('view-');
    const appOptions = { target: 'ES2022', module: 'NodeNext', moduleResolution: 'NodeNext', lib: ['ES2022'], types: [], baseUrl: '.', paths: { '@probe/lib': ['../lib'] }, noEmit: true, ...(controlled ? { noResolve: true, disableSolutionSearching: true } : {}) };
    const libOptions = { target: 'ES2022', module: 'NodeNext', moduleResolution: 'NodeNext', lib: ['ES2022'], types: [], composite: true, rootDir: 'src', outDir: 'dist', ...(controlled ? { noResolve: true } : {}) };
    await writeFiles(dir, {
      'tsconfig.json': { files: [], references: [{ path: './app' }, { path: './lib' }] },
      'app/tsconfig.json': { compilerOptions: appOptions, references: [{ path: '../lib' }], ...(controlled ? { files: ['src/main.ts', '../lib/src/index.ts'] } : { include: ['src'] }) },
      'lib/tsconfig.json': { compilerOptions: libOptions, ...(controlled ? { files: ['src/index.ts'] } : { include: ['src'] }) },
      'lib/package.json': { name: '@probe/lib', version: '1.0.0', types: 'dist/index.d.ts', main: 'dist/index.js' },
      'lib/src/index.ts': 'export class Service { save() { return "from-source"; } }\n',
      'app/src/main.ts': 'import { Service } from "@probe/lib";\nimport { dependency } from "probe-third-party";\nexport function run() { return new Service().save(); }\nexport function runThirdParty() { return dependency.save(); }\n',
      'node_modules/probe-third-party/package.json': { name: 'probe-third-party', version: '1.0.0', types: 'index.d.ts' },
      'node_modules/probe-third-party/index.d.ts': 'export declare const dependency: { save(): string };\n',
    });
    if (view) {
      await fs.unlink(path.join(dir, 'node_modules/probe-third-party/index.d.ts'));
      if (mode.includes('workspace')) {
        const appConfig = JSON.parse(await fs.readFile(path.join(dir, 'app/tsconfig.json'), 'utf8'));
        delete appConfig.compilerOptions.paths;
        delete appConfig.compilerOptions.baseUrl;
        await writeFiles(dir, { 'app/tsconfig.json': appConfig });
        await fs.mkdir(path.join(dir, 'node_modules/@probe'), { recursive: true });
        await fs.symlink('../../lib', path.join(dir, 'node_modules/@probe/lib'));
      }
    }
    const main = path.join(dir, 'app/src/main.ts'), lib = path.join(dir, 'lib/src/index.ts');
    if (mode === 'flat-external') {
      await server.custom('openExternalProject', { projectFileName: path.join(dir, 'narrant.external'), rootFiles: [main, lib].map(fileName => ({ fileName })), options: { ...base, noResolve: true, baseUrl: path.join(dir, 'app'), paths: { '@probe/lib': [path.join(dir, 'lib')] } }, typeAcquisition: { enable: false } });
    } else if (controlled || view) {
      await server.custom('openExternalProject', { projectFileName: path.join(dir, 'narrant.external'), rootFiles: ['app/tsconfig.json', 'lib/tsconfig.json'].map(f => ({ fileName: path.join(dir, f) })), options: base, typeAcquisition: { enable: false } });
    }
    await server.open(main);
    result.projects = [await server.custom('projectInfo', { file: main, needFileNameList: true })];
    result.local = await server.definition(main, 'Service().save', 10);
    result.thirdParty = await server.definition(main, 'dependency.save', 11);
    result.hierarchy = await server.hierarchy(main, 'run()');
  });
}

for (const projectNames of process.env.PROBE_REFERENCE_MODES ? [] : [['a', 'b'], ['a'], ['b']]) {
  await record(`shared-${projectNames.join('-')}`, async (server, result, dir) => {
    await writeFiles(dir, {
      'a/impl.ts': 'export class Service { save() { return "A"; } }\n',
      'b/impl.ts': 'export class Service { save() { return "B"; } }\n',
      'a/main.ts': 'import { shared } from "../shared";\nexport function fromA() { return shared(); }\n',
      'b/main.ts': 'import { shared } from "../shared";\nexport function fromB() { return shared(); }\n',
      'shared.ts': 'import { Service } from "@impl";\nexport function shared() { return new Service().save(); }\n',
    });
    for (const name of projectNames) {
      await server.custom('openExternalProject', {
        projectFileName: path.join(dir, `${name}.external`),
        rootFiles: [`${name}/main.ts`, `${name}/impl.ts`, 'shared.ts'].map(f => ({ fileName: path.join(dir, f) })),
        options: { ...base, noResolve: true, baseUrl: dir, paths: { '@impl': [path.join(dir, `${name}/impl.ts`)] } },
        typeAcquisition: { enable: false },
      });
    }
    const shared = path.join(dir, 'shared.ts');
    await server.open(shared);
    result.projects = [await server.custom('projectInfo', { file: shared, needFileNameList: true })];
    result.standardDefinition = await server.definition(shared, 'Service().save', 10);
    result.explicitDefinitions = [];
    const query = position(await fs.readFile(shared, 'utf8'), 'Service().save', 10);
    for (const name of projectNames) result.explicitDefinitions.push({
      project: name, response: await server.custom('definition', { file: shared, projectFileName: path.join(dir, `${name}.external`), line: query.line + 1, offset: query.character + 1 }),
    });
  });
}

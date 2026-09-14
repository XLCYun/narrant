import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { LspProbe, here, writeFiles } from './support.mjs';

const fixture = {
  'model.ts': 'export class Service { save() { return "ok"; } }\n',
  'barrel.ts': 'export { Service as LocalService } from "./model";\n',
  'main.ts': 'import { LocalService } from "./barrel";\nimport { thirdParty } from "probe-third-party";\nexport async function ownCall() { const item = await Promise.resolve(new LocalService()); return item.save(); }\nexport function dependencyCall() { return thirdParty.save(); }\n',
  'main.js': 'const { LocalService } = require("./barrel");\nfunction ownJsCall() { const item = new LocalService(); return item.save(); }\nmodule.exports = { ownJsCall };\n',
  'node_modules/probe-third-party/package.json': { name: 'probe-third-party', version: '1.0.0', types: 'index.d.ts' },
  'node_modules/probe-third-party/index.d.ts': 'export declare const thirdParty: { save(): string };\n',
};
const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-tls-scope-')));
const options = { target: 'es2022', module: 'commonjs', moduleResolution: 'node', lib: ['lib.es2022.d.ts'], allowJs: true, checkJs: false, types: [], noEmit: true, disableReferencedProjectLoad: true, disableSolutionSearching: true };
const cases = [];
for (const engine of ['ts5', 'ts6']) {
  for (const mode of ['normal', 'restricted', 'configured-restricted', 'open-first-restricted']) {
    const dir = path.join(root, `${engine}-${mode}`);
    await writeFiles(dir, fixture);
    if (mode.includes('configured') || mode.includes('open-first')) {
      await writeFiles(dir, { 'tsconfig.json': { compilerOptions: { target: 'ES2022', module: 'CommonJS', lib: ['ES2022'], allowJs: true, types: [] }, include: ['*.ts', '*.js'] } });
    }
    const server = new LspProbe(dir, engine === 'ts6' ? path.join(here, 'ts6/lib/tsserver.js') : path.resolve('node_modules/typescript/lib/tsserver.js'));
    const result = { engine, mode, dir };
    try {
      const initialized = await server.initialize();
      result.capabilities = initialized.capabilities;
      const main = path.join(dir, 'main.ts'), js = path.join(dir, 'main.js');
      if (mode.includes('open-first')) { await server.open(main); await server.open(js); result.beforeProject = await server.custom('projectInfo', { file: main, needFileNameList: true }); }
      result.external = await server.custom('openExternalProject', {
        projectFileName: path.join(dir, 'narrant.external'),
        rootFiles: ['model.ts', 'barrel.ts', 'main.ts', 'main.js'].map(name => ({ fileName: path.join(dir, name) })),
        options: { ...options, noResolve: mode !== 'normal' }, typeAcquisition: { enable: false },
      });
      if (!mode.includes('open-first')) { await server.open(main); await server.open(js); }
      result.project = await server.custom('projectInfo', { file: main, needFileNameList: true });
      result.localTs = await server.definition(main, 'item.save', 'item.'.length);
      result.localJs = await server.definition(js, 'item.save', 'item.'.length);
      result.thirdParty = await server.definition(main, 'thirdParty.save', 'thirdParty.'.length);
      result.hierarchy = await server.hierarchy(main, 'ownCall');
    } catch (error) { result.error = error.stack; }
    finally { result.lifecycle = await server.stop(); }
    cases.push(result);
    await fs.writeFile(path.join(here, 'scope-results.json'), JSON.stringify({ root, cases }, null, 2) + '\n');
    console.log(JSON.stringify({engine, mode, project: result.project?.body?.configFileName, fileCount: result.project?.body?.fileNames?.length, dependencies: result.project?.body?.fileNames?.filter(f => f.includes('probe-third-party')), localTs: result.localTs, localJs: result.localJs, thirdParty: result.thirdParty, error: result.error, exit: result.lifecycle.exit, remaining: result.lifecycle.remaining}));
  }
}

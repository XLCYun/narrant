import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import * as tar from 'tar';
import { ids, targets, validateCatalog, validateManifest, safePath, checksums, verifyAndExtract, downloadPacks } from '../codegraph/download-packs.mjs';
import { prepareDotnetFixtureReferences, validateToolchainManifest } from '../language-packs/prepare-test-sdks.mjs';
import { selectedToolchainNames } from '../language-packs/test-sdks.mjs';
import { validateCandidates, verificationCommands, validTestOutput } from '../codegraph/verify-consumer.mjs';

const hash = value => createHash('sha256').update(value).digest('hex');

/** 伪归档只用于验证下载器边界，不声称具备语言分析能力。 */
async function fixture(root, id = 'rust', target = 'aarch64-apple-darwin') {
  const input = path.join(root, 'input');
  await fs.mkdir(path.join(input,'bin'),{recursive:true});
  await fs.mkdir(path.join(input,'control'));
  const descriptor = JSON.stringify({schema_version:1,pack_id:id,pack_version:'0.1.0',engine_version:'0.1.0',protocol:'codegraph.analyzer/1'}) + '\n';
  const entry = 'bin/codegraph-' + id + (target === 'x86_64-pc-windows-msvc' ? '.exe' : '');
  await fs.writeFile(path.join(input,entry), 'fixture');
  await fs.chmod(path.join(input,entry), 0o755);
  await fs.writeFile(path.join(input,'control/codegraph.json'), descriptor);
  const archive = path.join(root,'pack.tar.gz');
  await tar.c({file:archive,cwd:input,gzip:true,portable:true,filter(name,stat) { stat.mode = (stat.mode & ~0o777) | (name === entry ? 0o755 : 0o644); return true; }},[entry,'control/codegraph.json']);
  const bytes = await fs.readFile(archive);
  const manifest = {schemaVersion:1,id,version:'0.1.0',target,engineVersion:'0.1.0',protocol:'codegraph.analyzer/1',sourceCommit:'a'.repeat(40),sourceDirty:false,archiveSize:bytes.length,archiveSha256:hash(bytes),files:[{path:entry,size:7,sha256:hash('fixture'),executable:true},{path:'control/codegraph.json',size:Buffer.byteLength(descriptor),sha256:hash(descriptor),executable:false}]};
  return {manifest,archive,input};
}

/** 仅替换外部发布目录和 HTTP 响应，真实下载、归档校验与落盘流程仍由生产代码执行。 */
async function releaseFixture(root, context, intercept = () => undefined) {
  const target = targets[process.platform + '-' + process.arch];
  assert.ok(target, '下载器验收在受支持的真实目标平台运行');
  const assets = new Map();
  for (const id of ids) {
    const { manifest, archive } = await fixture(path.join(root, 'fixtures', id), id, target);
    assets.set(id + '-' + target + '-manifest.json', Buffer.from(JSON.stringify(manifest) + '\n'));
    assets.set(id + '-' + target + '-pack.tar.gz', await fs.readFile(archive));
  }
  assets.set('SHA256SUMS', Buffer.from([...assets].map(([name, bytes]) => hash(bytes) + '  ' + name + '\n').join('')));
  const catalogPath = fileURLToPath(new URL('../../resources/language-support-packs.json', import.meta.url));
  const originalRead = fs.readFile.bind(fs);
  const catalog = JSON.parse(await originalRead(catalogPath));
  for (const id of ids) catalog.packs[id].published = true;
  context.mock.method(fs, 'readFile', (file, ...args) => file === catalogPath ? Promise.resolve(Buffer.from(JSON.stringify(catalog))) : originalRead(file, ...args));
  const requests = [];
  context.mock.method(globalThis, 'fetch', async url => {
    assert.equal(new URL(url).origin, 'https://github.com');
    assert.ok(new URL(url).pathname.startsWith('/XLCYun/CodeGraph/releases/download/v0.1.0/'));
    const name = new URL(url).pathname.split('/').at(-1);
    requests.push(name);
    const bytes = assets.get(name);
    assert.ok(bytes, name);
    return intercept(name, bytes) ?? new Response(bytes);
  });
  return { target, requests, assets };
}

/** 流在收到字节后中断，重试应复用已经验证的前一包，而不是被最终目录残片阻塞。 */
test('download retries in the same directory after a later package stream fails', { timeout: 10000 }, async context => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-retry-'));
  const output = path.join(root, 'output');
  let interrupt = true;
  try {
    const { target, requests } = await releaseFixture(root, context, (name, bytes) => {
      if (!interrupt || !name.startsWith('typescript-javascript-') || !name.endsWith('pack.tar.gz')) return;
      interrupt = false;
      let sent = false;
      return new Response(new ReadableStream({ pull(controller) {
        if (sent) controller.error(new Error('fixture interrupted download'));
        else { sent = true; controller.enqueue(bytes.subarray(0, 8)); }
      } }));
    });
    await assert.rejects(downloadPacks(output), /fixture interrupted download/);
    await assert.rejects(fs.access(path.join(output, 'candidates.json')), { code: 'ENOENT' });
    await downloadPacks(output);
    const candidates = JSON.parse(await fs.readFile(path.join(output, 'candidates.json')));
    validateCandidates(candidates, target);
    for (const item of candidates) assert.equal(await fs.readFile(path.join(item.root, 'bin/codegraph-' + item.id + (target === 'x86_64-pc-windows-msvc' ? '.exe' : '')), 'utf8'), 'fixture');
    assert.equal(requests.filter(name => name === 'rust-' + target + '-pack.tar.gz').length, 1);
    assert.equal((await fs.readdir(output)).some(name => name.startsWith('.download-')), false);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** 兼容旧下载器遗留的最终目录残片；成功后也必须重新验证缓存，不能以存在代替完整性。 */
test('download replaces incomplete legacy output and revalidates every cached file', async context => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-cache-'));
  const output = path.join(root, 'output');
  let unavailable = false;
  try {
    const { target, requests } = await releaseFixture(root, context, name => {
      if (unavailable && name.startsWith('rust-') && name.endsWith('manifest.json')) return new Response('', { status: 503 });
    });
    const packageRoot = path.join(output, 'rust', '0.1.0', target);
    await fs.mkdir(packageRoot, { recursive: true });
    await fs.writeFile(path.join(packageRoot, 'manifest.json'), '{partial');
    await fs.writeFile(path.join(packageRoot, 'pack.tar.gz'), 'partial');
    await fs.mkdir(path.join(output, 'installed', 'rust'), { recursive: true });
    await downloadPacks(output);
    const index = path.join(output, 'candidates.json');
    const complete = await fs.readFile(index, 'utf8');
    await downloadPacks(output);
    assert.equal(await fs.readFile(index, 'utf8'), complete);
    assert.equal(requests.filter(name => name.endsWith('pack.tar.gz')).length, 7);
    const rust = JSON.parse(complete).find(item => item.id === 'rust');
    const entry = path.join(rust.root, 'bin/codegraph-rust' + (target === 'x86_64-pc-windows-msvc' ? '.exe' : ''));
    await fs.writeFile(entry, 'corrupt');
    await downloadPacks(output);
    assert.equal(await fs.readFile(entry, 'utf8'), 'fixture');
    await fs.writeFile(path.join(rust.root, 'control', 'unexpected.json'), '{}');
    await downloadPacks(output);
    await assert.rejects(fs.access(path.join(rust.root, 'control', 'unexpected.json')), { code: 'ENOENT' });
    assert.equal(requests.filter(name => name === 'rust-' + target + '-pack.tar.gz').length, 3);
    assert.equal((await fs.readdir(output)).some(name => name.startsWith('.download-')), false);
    await fs.writeFile(entry, 'corrupt');
    unavailable = true;
    await assert.rejects(downloadPacks(output), /download failed: 503/);
    await assert.rejects(fs.access(index), { code: 'ENOENT' });
    unavailable = false;
    await downloadPacks(output);
    validateCandidates(JSON.parse(await fs.readFile(index)), target);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** 整包提升失败时恢复先前目录；下一次执行仍可完成同一路径的替换。 */
test('download restores previous output if package promotion fails and remains retryable', async context => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-promotion-'));
  const output = path.join(root, 'output');
  try {
    const { target } = await releaseFixture(root, context);
    const packageRoot = path.join(output, 'rust', '0.1.0', target);
    await fs.mkdir(packageRoot, { recursive: true });
    await fs.writeFile(path.join(packageRoot, 'manifest.json'), 'previous incomplete download');
    const rename = fs.rename.bind(fs);
    let fail = true;
    context.mock.method(fs, 'rename', (source, destination) => {
      if (fail && destination === packageRoot && path.basename(source) === 'rust') {
        fail = false;
        return Promise.reject(new Error('fixture promotion failed'));
      }
      return rename(source, destination);
    });
    await assert.rejects(downloadPacks(output), /fixture promotion failed/);
    assert.equal(await fs.readFile(path.join(packageRoot, 'manifest.json'), 'utf8'), 'previous incomplete download');
    await assert.rejects(fs.access(path.join(output, 'candidates.json')), { code: 'ENOENT' });
    assert.equal((await fs.readdir(output)).some(name => name.startsWith('.download-')), false);
    await downloadPacks(output);
    validateCandidates(JSON.parse(await fs.readFile(path.join(output, 'candidates.json'))), target);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** 发布摘要或解包文件摘要失败都不发布半成品索引，修复输入后可在原目录重新运行。 */
for (const failure of ['release checksum', 'extracted checksum']) {
  test('download retries after ' + failure + ' failure without exposing partial candidates', async context => {
    const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-checksum-retry-'));
    const output = path.join(root, 'output');
    let corrupt = failure === 'release checksum';
    try {
      const { target, assets } = await releaseFixture(root, context, (name, bytes) => {
        if (!corrupt || !name.startsWith('typescript-javascript-') || !name.endsWith('pack.tar.gz')) return;
        corrupt = false;
        return new Response(bytes.subarray(0, bytes.length - 1));
      });
      const manifestName = 'typescript-javascript-' + target + '-manifest.json';
      const originalManifest = assets.get(manifestName);
      const originalSums = assets.get('SHA256SUMS');
      if (failure === 'extracted checksum') {
        const manifest = JSON.parse(originalManifest);
        manifest.files[0].sha256 = '0'.repeat(64);
        assets.set(manifestName, Buffer.from(JSON.stringify(manifest) + '\n'));
        assets.set('SHA256SUMS', Buffer.from([...assets].filter(([name]) => name !== 'SHA256SUMS').map(([name, bytes]) => hash(bytes) + '  ' + name + '\n').join('')));
      }
      await assert.rejects(downloadPacks(output), /checksum/);
      await assert.rejects(fs.access(path.join(output, 'candidates.json')), { code: 'ENOENT' });
      await assert.rejects(fs.access(path.join(output, 'typescript-javascript', '0.1.0', target)), { code: 'ENOENT' });
      assert.equal((await fs.readdir(output)).some(name => name.startsWith('.download-')), false);
      assets.set(manifestName, originalManifest);
      assets.set('SHA256SUMS', originalSums);
      await downloadPacks(output);
      validateCandidates(JSON.parse(await fs.readFile(path.join(output, 'candidates.json'))), target);
    } finally { await fs.rm(root, { recursive: true, force: true }); }
  });
}

test('fixed release catalog fails while unpublished and never selects a floating release', async () => {
  const catalog = JSON.parse(await fs.readFile(new URL('../../resources/language-support-packs.json', import.meta.url)));
  for (const id of ids) catalog.packs[id].published = false;
  assert.throws(() => validateCatalog(catalog), /unpublished/);
  for (const id of ids) catalog.packs[id].published = true;
  validateCatalog(catalog);
  catalog.tag='latest';
  assert.throws(() => validateCatalog(catalog), /identity/);
});

/** 每包必须提供自身目标平台入口，其他语言和旧总入口不能替代它。 */
test('all seven manifests require their fixed executable entry on both targets', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-entry-'));
  try {
    const { manifest } = await fixture(root);
    const encode = value => Buffer.from(JSON.stringify(value) + '\n');
    for (const target of ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc']) {
      const suffix = target === 'x86_64-pc-windows-msvc' ? '.exe' : '';
      for (const id of ids) {
        const value = structuredClone(manifest);
        value.id = id;
        value.target = target;
        value.files[0].path = 'bin/codegraph-' + id + suffix;
        validateManifest(encode(value), id, target);
        for (const wrong of ['bin/codegraph' + suffix, 'bin/codegraph-' + (id === 'rust' ? 'python' : 'rust') + suffix, 'bin/codegraph-' + id + (suffix ? '' : '.exe')]) {
          assert.throws(() => validateManifest(encode({ ...value, files: [{ ...value.files[0], path: wrong }, value.files[1]] }), id, target), /Missing public/);
        }
        assert.throws(() => validateManifest(encode({ ...value, files: [{ ...value.files[0], executable: false }, value.files[1]] }), id, target), /Missing public/);
        assert.throws(() => validateManifest(encode({ ...value, files: [value.files[0]] }), id, target), /Missing public/);
      }
    }
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

test('candidate configuration accepts only public identities and rejects absent selected packs', () => {
  const value = {id:'rust',version:'0.1.0',engineVersion:'0.1.0',protocol:'codegraph.analyzer/1',target:'aarch64-apple-darwin',root:path.resolve('/tmp/candidate')};
  validateCandidates([value], value.target, 'rust');
  assert.throws(() => validateCandidates([value],value.target,'all'), /Missing/);
  assert.throws(() => validateCandidates([{...value,environment:{}}],value.target,'rust'), /identity/);
  assert.throws(() => validateCandidates([value,value],value.target,'rust'), /identity/);
});

test('download boundary rejects unsafe names, checksum ambiguity, and incompatible provenance', async () => {
  for (const name of ['../x','/x','bin/../x','bin/CON','bin/x:stream','bin/x.','bin\\x']) assert.equal(safePath(name),false,name);
  assert.equal(safePath('runtime/private-helper/data.json'),true);
  assert.throws(()=>checksums(Buffer.from(`${'a'.repeat(64)}  a\n${'b'.repeat(64)}  a\n`)),/duplicate/);
  const root=await fs.mkdtemp(path.join(os.tmpdir(),'codegraph-test-'));
  try {
    const {manifest}=await fixture(root);
    const encode=value=>Buffer.from(JSON.stringify(value)+'\n');
    validateManifest(encode(manifest),'rust',manifest.target);
    for (const [key,value] of [['sourceDirty',true],['engineVersion','0.2.0'],['protocol','bad'],['sourceCommit','short']]) assert.throws(()=>validateManifest(encode({...manifest,[key]:value}),'rust',manifest.target));
    assert.throws(()=>validateManifest(Buffer.from(JSON.stringify(manifest).replace('"sourceDirty":false','"sourceDirty":false,"sourceDirty":false')+'\n'),'rust',manifest.target),/canonical/);
  } finally {await fs.rm(root,{recursive:true,force:true});}
});

test('real tar extraction verifies exact inventory and file hashes before exposing a candidate', async () => {
  const root=await fs.mkdtemp(path.join(os.tmpdir(),'codegraph-tar-'));
  try {
    const {manifest,archive}=await fixture(root);
    await verifyAndExtract(manifest,archive,path.join(root,'installed'));
    assert.equal(await fs.readFile(path.join(root,'installed/bin/codegraph-rust'),'utf8'),'fixture');
    const changed=structuredClone(manifest);changed.files[0].sha256='0'.repeat(64);
    await assert.rejects(verifyAndExtract(changed,archive,path.join(root,'corrupt')),/checksum/);
    const missing=structuredClone(manifest);missing.files.pop();
    await assert.rejects(verifyAndExtract(missing,archive,path.join(root,'extra')),/entries/);
  } finally {await fs.rm(root,{recursive:true,force:true});}
});

test('consumer verification keeps all 38 diff tests and all language installation and Review entrypoints', async () => {
  const commands = verificationCommands('all','darwin');
  const joined=JSON.stringify(commands);
  assert.doesNotMatch(joined,/native-helpers|--build|native_languages|native_analyzer/);
  for(const filter of ['rust_graph','rust_syntax','rust_production','rust_lifecycle','python_graph','python_native','typescript_graph','codegraph_graphs','real_codegraph_packs_install','real_released_python_pack_downloads_on_demand_and_analyzes','real_rust_pack_flows','real_python_pack_flows','real_typescript_javascript_graphs_flow','real_cli_exit_and_cancellation']) assert.ok(joined.includes(filter),filter);
  for (const platform of ['darwin', 'win32']) for (const language of ['all', 'python']) {
    assert.ok(JSON.stringify(verificationCommands(language, platform)).includes('real_released_python_pack_downloads_on_demand_and_analyzes'));
  }
  const inventory=JSON.parse(await fs.readFile(new URL('../../resources/codegraph-consumer-acceptance.json',import.meta.url)));
  assert.equal(inventory.diffTests.length,38);
  for(const item of inventory.diffTests) assert.match(await fs.readFile(new URL(`../../${item.new}`,import.meta.url),'utf8'),new RegExp(`fn ${item.test}\\(`));
  for(const command of commands) {assert.equal(command.executable,'bash');assert.equal(command.args[0],'scripts/cargo-agent.sh');}
});

test('CI consumes exact release bytes on both hosted targets without helper builds or publication', async () => {
  const workflow=await fs.readFile(new URL('../../.github/workflows/validate-language-support-packs.yml',import.meta.url),'utf8');
  for(const expected of ['macos-15','windows-2022','download-packs.mjs','verify-consumer.mjs','prepare-test-sdks.mjs','rust-src']) assert.ok(workflow.includes(expected),expected);
  assert.doesNotMatch(workflow,/native-helpers|publish:|contents: write|continue-on-error|build-language-support-packs|--build/);
});

/** 沿用真实安装目录布局，消费者只准备目标项目引用，不拼装 helper platform。 */
test('target SDK fixtures stay separate from helper build tools and preserve .NET reference layout', async () => {
  const manifest = JSON.parse(await fs.readFile(new URL('../../resources/codegraph-test-sdks.json',import.meta.url)));
  validateToolchainManifest(manifest);
  assert.deepEqual(selectedToolchainNames('java'), ['jdk25','jdk17','jdk8']);
  assert.deepEqual(selectedToolchainNames('go'), ['go','go-target']);
  assert.deepEqual(selectedToolchainNames('csharp'), ['dotnet','dotnet-ref8','dotnet-ref9']);
  assert.deepEqual(selectedToolchainNames('c-cpp'), []);
  assert.equal(selectedToolchainNames().length,8);
  assert.throws(()=>selectedToolchainNames('none'), /Unknown/);
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'codegraph-references-'));
  try {
    const sdk = path.join(root,'sdk'), source = path.join(root,'source');
    await fs.mkdir(path.join(source,'ref/net9.0'), {recursive:true});
    await fs.writeFile(path.join(source,'ref/net9.0/System.Runtime.dll'),'reference');
    await fs.writeFile(path.join(source,'Microsoft.NETCore.App.Ref.nuspec'),'<version>9.0.20</version>');
    await fs.mkdir(path.join(source,'build'));
    await fs.writeFile(path.join(source,'build/untrusted.targets'),'do not copy');
    await prepareDotnetFixtureReferences({NARRANT_DOTNET_SDK:sdk,NARRANT_DOTNET_REF9_ROOT:source});
    assert.equal(await fs.readFile(path.join(sdk,'packs/Microsoft.NETCore.App.Ref/9.0.20/ref/net9.0/System.Runtime.dll'),'utf8'),'reference');
    await assert.rejects(fs.stat(path.join(sdk,'packs/Microsoft.NETCore.App.Ref/9.0.20/build')), {code:'ENOENT'});
  } finally {await fs.rm(root,{recursive:true,force:true});}
});

/** 摘要中的正数不能替代迁移清单要求的逐项真实执行。 */
test('consumer test receipt requires every retained named success and refuses zero tests', () => {
  const name = 'structure_review::codegraph_client::tests::typescript_graph::graph_diff_format_child_parent_and_inline_callback';
  const output = `test ${name} ... ok\ntest result: ok. 1 passed; 0 failed;\n`;
  assert.equal(validTestOutput(output, [name]), true);
  assert.equal(validTestOutput(output, ['missing']), false);
  assert.equal(validTestOutput('test result: ok. 0 passed; 0 failed;', []), false);
  const required = verificationCommands('all','darwin').flatMap(command => command.requiredTests);
  assert.equal(required.length,38);
  assert.equal(new Set(required).size,38);
});

import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import test from 'node:test';
import * as tar from 'tar';
import { ids, validateCatalog, validateManifest, safePath, checksums, verifyAndExtract } from '../codegraph/download-packs.mjs';
import { prepareDotnetFixtureReferences, validateToolchainManifest } from '../language-packs/prepare-test-sdks.mjs';
import { selectedToolchainNames } from '../language-packs/test-sdks.mjs';
import { validateCandidates, verificationCommands, validTestOutput } from '../codegraph/verify-consumer.mjs';

const hash = value => createHash('sha256').update(value).digest('hex');

/** 伪归档只用于验证下载器边界，不声称具备语言分析能力。 */
async function fixture(root) {
  const input = path.join(root, 'input');
  await fs.mkdir(path.join(input,'bin'),{recursive:true});
  await fs.mkdir(path.join(input,'control'));
  const descriptor = JSON.stringify({schema_version:1,pack_id:'rust',pack_version:'0.1.0',engine_version:'0.1.0',protocol:'codegraph.analyzer/1'}) + '\n';
  await fs.writeFile(path.join(input,'bin/codegraph'), 'fixture');
  await fs.chmod(path.join(input,'bin/codegraph'), 0o755);
  await fs.writeFile(path.join(input,'control/codegraph.json'), descriptor);
  const archive = path.join(root,'pack.tar.gz');
  await tar.c({file:archive,cwd:input,gzip:true,portable:true,filter(name,stat) { stat.mode = (stat.mode & ~0o777) | (name === 'bin/codegraph' ? 0o755 : 0o644); return true; }},['bin/codegraph','control/codegraph.json']);
  const bytes = await fs.readFile(archive);
  const manifest = {schemaVersion:1,id:'rust',version:'0.1.0',target:'aarch64-apple-darwin',engineVersion:'0.1.0',protocol:'codegraph.analyzer/1',sourceCommit:'a'.repeat(40),sourceDirty:false,archiveSize:bytes.length,archiveSha256:hash(bytes),files:[{path:'bin/codegraph',size:7,sha256:hash('fixture'),executable:true},{path:'control/codegraph.json',size:Buffer.byteLength(descriptor),sha256:hash(descriptor),executable:false}]};
  return {manifest,archive,input};
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
    assert.equal(await fs.readFile(path.join(root,'installed/bin/codegraph'),'utf8'),'fixture');
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
  for(const filter of ['rust_graph','rust_syntax','rust_production','python_graph','python_native','typescript_graph','codegraph_graphs','real_codegraph_packs_install','real_rust_pack_flows','real_python_pack_flows','real_typescript_javascript_graphs_flow','real_cli_exit_and_cancellation']) assert.ok(joined.includes(filter),filter);
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
